#!/usr/bin/env python3
"""Write the Windows portable package ZIP with explicit, stable metadata."""

from __future__ import annotations

import argparse
import datetime as dt
import os
import platform
import re
import shutil
import stat
import sys
import tempfile
import zipfile
import zlib
from pathlib import Path, PurePosixPath


class ZipBuildError(ValueError):
    """Raised when the input tree cannot be represented safely in the ZIP."""


FORBIDDEN_PACKAGE_PATH = re.compile(r"(^|/)(include|lib|Headers|cmake)(/|$)|\.(a|lib|prl|so)$")


def toolchain_identity() -> str:
    return (
        f"python-zipfile-{platform.python_version()}"
        f"-zlib-{zlib.ZLIB_VERSION}-runtime-{zlib.ZLIB_RUNTIME_VERSION}"
    )


def _timestamp(epoch: int) -> tuple[int, int, int, int, int, int]:
    try:
        value = dt.datetime.fromtimestamp(epoch, tz=dt.timezone.utc)
    except (OverflowError, OSError, ValueError) as error:
        raise ZipBuildError(f"invalid SOURCE_DATE_EPOCH {epoch}: {error}") from error
    if value.year < 1980 or value.year > 2107:
        raise ZipBuildError("SOURCE_DATE_EPOCH must map to a ZIP-compatible year (1980..2107)")
    # DOS timestamps have two-second resolution. Round down explicitly.
    return value.year, value.month, value.day, value.hour, value.minute, value.second - value.second % 2


def _archive_name(path: Path) -> str:
    name = path.as_posix()
    pure = PurePosixPath(name)
    if not name or pure.is_absolute() or any(part in ("", ".", "..") for part in pure.parts):
        raise ZipBuildError(f"unsafe package path: {name!r}")
    if "\\" in name or ":" in name or "\x00" in name:
        raise ZipBuildError(f"non-portable package path: {name!r}")
    return name


def _collect_files(root: Path) -> list[tuple[str, Path]]:
    if not root.is_dir():
        raise ZipBuildError(f"package root is not a directory: {root}")

    files: list[tuple[str, Path]] = []
    for path in root.rglob("*"):
        if path.is_symlink():
            raise ZipBuildError(f"refusing symlink in package tree: {path.relative_to(root)}")
        if path.is_dir():
            continue
        if not path.is_file():
            raise ZipBuildError(f"refusing non-regular package entry: {path.relative_to(root)}")
        relative = _archive_name(path.relative_to(root))
        files.append((relative, path))

    files.sort(key=lambda item: item[0].encode("utf-8"))
    if not files:
        raise ZipBuildError("package tree is empty")

    folded: set[str] = set()
    for name, _ in files:
        key = name.casefold()
        if key in folded:
            raise ZipBuildError(f"case-insensitive duplicate package path: {name}")
        folded.add(key)
    return files


def _verify_archive(
    archive_path: Path,
    names: list[str],
    required: set[str],
    timestamp: tuple[int, int, int, int, int, int],
    executables: set[str],
) -> None:
    with zipfile.ZipFile(archive_path, "r") as archive:
        infos = archive.infolist()
        actual_names = [info.filename for info in infos]
        if actual_names != names:
            raise ZipBuildError("archive member order or names changed during ZIP creation")
        if len(actual_names) != len(set(actual_names)):
            raise ZipBuildError("archive contains duplicate file names")
        missing = required.difference(actual_names)
        if missing:
            raise ZipBuildError(f"archive is missing required files: {', '.join(sorted(missing))}")
        forbidden = [name for name in actual_names if FORBIDDEN_PACKAGE_PATH.search(name)]
        if forbidden:
            raise ZipBuildError(f"archive contains a development/SDK path: {forbidden[0]}")
        if archive.comment:
            raise ZipBuildError("archive contains an unexpected comment")
        for info in infos:
            if info.date_time != timestamp:
                raise ZipBuildError(f"archive timestamp was not normalized: {info.filename}")
            if info.create_system != 3 or info.extra or info.comment or info.internal_attr:
                raise ZipBuildError(f"archive metadata was not normalized: {info.filename}")
            expected_mode = 0o755 if info.filename in executables else 0o644
            actual_mode = (info.external_attr >> 16) & 0o777
            if actual_mode != expected_mode:
                raise ZipBuildError(f"archive mode was not normalized: {info.filename}")
        corrupt = archive.testzip()
        if corrupt is not None:
            raise ZipBuildError(f"archive CRC check failed: {corrupt}")


def build_archive(
    root: Path,
    output: Path,
    source_date_epoch: int,
    executables: set[str],
    required_files: set[str],
) -> None:
    root = root.resolve(strict=True)
    output = output.resolve()
    try:
        output.relative_to(root)
    except ValueError:
        pass
    else:
        raise ZipBuildError("archive output must be outside the package input tree")

    timestamp = _timestamp(source_date_epoch)
    files = _collect_files(root)
    names = [name for name, _ in files]
    known = set(names)
    missing = required_files.difference(known)
    if missing:
        raise ZipBuildError(f"package tree is missing required files: {', '.join(sorted(missing))}")

    executable_names = {_archive_name(Path(name)) for name in executables}
    if not executable_names.issubset(known):
        raise ZipBuildError("an executable entry is absent from the package tree")
    required = {_archive_name(Path(name)) for name in required_files}
    output.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{output.name}.", suffix=".tmp", dir=output.parent)
    os.close(descriptor)
    temporary = Path(temporary_name)

    try:
        with zipfile.ZipFile(
            temporary,
            mode="w",
            compression=zipfile.ZIP_DEFLATED,
            compresslevel=9,
            strict_timestamps=True,
        ) as archive:
            archive.comment = b""
            for name, source in files:
                info = zipfile.ZipInfo(filename=name, date_time=timestamp)
                info.create_system = 3
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = (stat.S_IFREG | (0o755 if name in executable_names else 0o644)) << 16
                info.internal_attr = 0
                info.extra = b""
                info.comment = b""
                with source.open("rb") as source_file, archive.open(info, "w") as destination:
                    shutil.copyfileobj(source_file, destination, length=1024 * 1024)

        _verify_archive(temporary, names, required, timestamp, executable_names)
        os.replace(temporary, output)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--source-date-epoch", required=True, type=int)
    parser.add_argument("--executable", action="append", default=[])
    parser.add_argument("--required-file", action="append", default=[])
    return parser


def main() -> int:
    arguments = _parser().parse_args()
    try:
        build_archive(
            root=arguments.root,
            output=arguments.output,
            source_date_epoch=arguments.source_date_epoch,
            executables=set(arguments.executable),
            required_files=set(arguments.required_file),
        )
    except (OSError, ZipBuildError, zipfile.BadZipFile) as error:
        print(f"[synveil-reproducible-zip] ERROR: {error}", file=sys.stderr)
        return 1

    print(f"[synveil-reproducible-zip] toolchain={toolchain_identity()}")
    print(f"[synveil-reproducible-zip] created={arguments.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
