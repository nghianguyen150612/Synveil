#!/usr/bin/env python3
"""Regression tests for deterministic Windows portable ZIP output."""

from __future__ import annotations

import datetime as dt
import importlib.util
import os
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[2]
MODULE_SPEC = importlib.util.spec_from_file_location(
    "create_reproducible_zip", ROOT / "scripts/create-reproducible-zip.py"
)
assert MODULE_SPEC is not None and MODULE_SPEC.loader is not None
ZIP_BUILDER = importlib.util.module_from_spec(MODULE_SPEC)
sys.modules[MODULE_SPEC.name] = ZIP_BUILDER
MODULE_SPEC.loader.exec_module(ZIP_BUILDER)
ZipBuildError = ZIP_BUILDER.ZipBuildError
build_archive = ZIP_BUILDER.build_archive


REQUIRED = {
    "synveil-desktop.exe",
    "synveil-client.exe",
    "qt.conf",
    "platforms/qwindows.dll",
    "LICENSE",
    "NOTICE",
    "SYNVEIL-MANIFEST.txt",
}
EXECUTABLES = {"synveil-desktop.exe", "synveil-client.exe"}
SOURCE_DATE_EPOCH = 1_777_777_777


def write_fixture(root: Path, reverse: bool, metadata_offset: int) -> dict[str, bytes]:
    payloads = {
        "synveil-desktop.exe": b"desktop executable payload\x00\x01",
        "synveil-client.exe": b"client executable payload\x02\x03",
        "qt.conf": b"[Paths]\nPrefix=.\n",
        "platforms/qwindows.dll": b"platform plugin payload\x04\x05",
        "LICENSE": b"license\n",
        "NOTICE": b"notice\n",
        "SYNVEIL-MANIFEST.txt": b"manifest\n",
        "qml/Controls/Button.qml": b"import QtQuick\nItem {}\n",
    }
    items = list(payloads.items())
    if reverse:
        items.reverse()
    for index, (relative, contents) in enumerate(items):
        destination = root / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(contents)
        changed_time = SOURCE_DATE_EPOCH + metadata_offset + index * 7
        os.utime(destination, (changed_time, changed_time))
        try:
            os.chmod(destination, 0o600 if index % 2 else 0o777)
        except OSError:
            pass
    return payloads


class ReproducibleZipTests(unittest.TestCase):
    def test_independent_trees_have_identical_sorted_archives_and_metadata(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synveil-zip-regression-") as directory:
            temp = Path(directory)
            root_a = temp / "tree-a"
            root_b = temp / "tree-b"
            root_a.mkdir()
            root_b.mkdir()
            payloads_a = write_fixture(root_a, reverse=False, metadata_offset=0)
            write_fixture(root_b, reverse=True, metadata_offset=101)
            archive_a = temp / "a.zip"
            archive_b = temp / "b.zip"

            build_archive(root_a, archive_a, SOURCE_DATE_EPOCH, EXECUTABLES, REQUIRED)
            build_archive(root_b, archive_b, SOURCE_DATE_EPOCH, EXECUTABLES, REQUIRED)

            self.assertEqual(archive_a.read_bytes(), archive_b.read_bytes())
            expected_names = sorted(payloads_a, key=lambda name: name.encode("utf-8"))
            timestamp = dt.datetime.fromtimestamp(SOURCE_DATE_EPOCH, tz=dt.timezone.utc)
            expected_time = timestamp.replace(second=timestamp.second // 2 * 2)
            expected_timestamp = (
                expected_time.year,
                expected_time.month,
                expected_time.day,
                expected_time.hour,
                expected_time.minute,
                expected_time.second,
            )
            with zipfile.ZipFile(archive_a) as archive:
                infos = archive.infolist()
                self.assertEqual([info.filename for info in infos], expected_names)
                self.assertEqual(archive.testzip(), None)
                for info in infos:
                    self.assertEqual(info.date_time, expected_timestamp)
                    self.assertEqual(info.create_system, 3)
                    self.assertEqual(info.extra, b"")
                    self.assertEqual(info.comment, b"")
                    self.assertEqual(info.internal_attr, 0)
                    self.assertEqual(info.compress_type, zipfile.ZIP_DEFLATED)
                    mode = (info.external_attr >> 16) & 0o777
                    self.assertEqual(mode, 0o755 if info.filename in EXECUTABLES else 0o644)
                    self.assertEqual(archive.read(info.filename), payloads_a[info.filename])

    def test_missing_required_member_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synveil-zip-missing-") as directory:
            root = Path(directory) / "root"
            root.mkdir()
            (root / "only.txt").write_text("fixture\n", encoding="utf-8")
            with self.assertRaisesRegex(ZipBuildError, "missing required files"):
                build_archive(
                    root,
                    Path(directory) / "out.zip",
                    SOURCE_DATE_EPOCH,
                    set(),
                    {"synveil-desktop.exe"},
                )

    def test_output_inside_package_tree_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory(prefix="synveil-zip-output-") as directory:
            root = Path(directory)
            (root / "file.txt").write_text("fixture\n", encoding="utf-8")
            with self.assertRaisesRegex(ZipBuildError, "outside the package input tree"):
                build_archive(
                    root,
                    root / "package.zip",
                    SOURCE_DATE_EPOCH,
                    set(),
                    set(),
                )

    def test_windows_alternate_data_stream_paths_are_rejected(self) -> None:
        with self.assertRaisesRegex(ZipBuildError, "non-portable package path"):
            ZIP_BUILDER._archive_name(PurePosixPath("qml/Controls/Widget.qml:stream"))


if __name__ == "__main__":
    unittest.main(verbosity=2)
