#!/usr/bin/env python3
"""Report PE header/section and MSVC intermediate differences between builds."""

from __future__ import annotations

import hashlib
import struct
import sys
from pathlib import Path
def _u16(data: bytes, offset: int) -> int:
    return struct.unpack_from("<H", data, offset)[0]


def _u32(data: bytes, offset: int) -> int:
    return struct.unpack_from("<I", data, offset)[0]


def _u64(data: bytes, offset: int) -> int:
    return struct.unpack_from("<Q", data, offset)[0]


def inspect_pe(path: Path) -> tuple[dict[str, int | str], dict[str, str], list[tuple[int, int, str]]]:
    data = path.read_bytes()
    if len(data) < 0x40 or data[:2] != b"MZ":
        raise ValueError("missing DOS MZ header")
    pe_offset = _u32(data, 0x3C)
    if pe_offset + 24 > len(data) or data[pe_offset : pe_offset + 4] != b"PE\0\0":
        raise ValueError("missing PE signature or COFF header")

    coff = pe_offset + 4
    machine = _u16(data, coff)
    section_count = _u16(data, coff + 2)
    optional_size = _u16(data, coff + 16)
    optional = coff + 20
    if optional + optional_size > len(data) or optional_size < 68:
        raise ValueError("truncated optional header")
    magic = _u16(data, optional)
    if magic == 0x20B:
        image_base_offset, directory_count_offset, directory_offset = 24, 108, 112
        image_base = _u64(data, optional + image_base_offset)
    elif magic == 0x10B:
        image_base_offset, directory_count_offset, directory_offset = 28, 92, 96
        image_base = _u32(data, optional + image_base_offset)
    else:
        raise ValueError(f"unsupported optional-header magic 0x{magic:04x}")

    fields: dict[str, int | str] = {
        "dos.pe_offset": pe_offset,
        "coff.machine": machine,
        "coff.section_count": section_count,
        "coff.timestamp": _u32(data, coff + 4),
        "coff.symbol_table_pointer": _u32(data, coff + 8),
        "coff.symbol_count": _u32(data, coff + 12),
        "coff.optional_header_size": optional_size,
        "coff.characteristics": _u16(data, coff + 18),
        "optional.magic": magic,
        "optional.linker_major": data[optional + 2],
        "optional.linker_minor": data[optional + 3],
        "optional.code_size": _u32(data, optional + 4),
        "optional.entrypoint": _u32(data, optional + 16),
        "optional.image_base": image_base,
        "optional.section_alignment": _u32(data, optional + 32),
        "optional.file_alignment": _u32(data, optional + 36),
        "optional.size_of_image": _u32(data, optional + 56),
        "optional.size_of_headers": _u32(data, optional + 60),
        "optional.checksum": _u32(data, optional + 64),
        "optional.subsystem": _u16(data, optional + 68),
        "optional.dll_characteristics": _u16(data, optional + 70),
    }
    directory_count = min(_u32(data, optional + directory_count_offset), 16)
    for index in range(directory_count):
        entry = optional + directory_offset + index * 8
        if entry + 8 > optional + optional_size:
            break
        fields[f"directory[{index}].rva"] = _u32(data, entry)
        fields[f"directory[{index}].size"] = _u32(data, entry + 4)

    section_table = optional + optional_size
    section_hashes: dict[str, str] = {}
    section_ranges: list[tuple[int, int, str]] = []
    for index in range(section_count):
        entry = section_table + index * 40
        if entry + 40 > len(data):
            raise ValueError("truncated section table")
        raw_name = data[entry : entry + 8].split(b"\0", 1)[0]
        name = raw_name.decode("ascii", errors="replace") or f"section-{index}"
        virtual_size = _u32(data, entry + 8)
        virtual_address = _u32(data, entry + 12)
        raw_size = _u32(data, entry + 16)
        raw_pointer = _u32(data, entry + 20)
        characteristics = _u32(data, entry + 36)
        fields[f"section[{index}].name"] = name
        fields[f"section[{index}].virtual_size"] = virtual_size
        fields[f"section[{index}].virtual_address"] = virtual_address
        fields[f"section[{index}].raw_size"] = raw_size
        fields[f"section[{index}].raw_pointer"] = raw_pointer
        fields[f"section[{index}].characteristics"] = characteristics
        if raw_size:
            end = raw_pointer + raw_size
            if end > len(data):
                raise ValueError(f"section {name} extends beyond the image")
            section_hashes[name] = hashlib.sha256(data[raw_pointer:end]).hexdigest()
            section_ranges.append((raw_pointer, end, name))
    return fields, section_hashes, section_ranges


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def first_different_byte(left: Path, right: Path) -> int | None:
    offset = 0
    with left.open("rb") as a, right.open("rb") as b:
        while True:
            a_block, b_block = a.read(1024 * 1024), b.read(1024 * 1024)
            if a_block != b_block:
                for index, (a_byte, b_byte) in enumerate(zip(a_block, b_block)):
                    if a_byte != b_byte:
                        return offset + index
                return offset + min(len(a_block), len(b_block))
            if not a_block:
                return None
            offset += len(a_block)


def compare_pe(build_a: Path, build_b: Path, name: str, output: list[str]) -> None:
    left, right = build_a / "release" / f"{name}.exe", build_b / "release" / f"{name}.exe"
    if not left.is_file() or not right.is_file():
        output.append(f"PE {name}: MISSING (build-a={left.is_file()} build-b={right.is_file()})")
        return
    output.append(f"PE {name}: size-a={left.stat().st_size} size-b={right.stat().st_size}")
    output.append(f"  sha256-a={sha256_file(left)}")
    output.append(f"  sha256-b={sha256_file(right)}")
    offset = first_different_byte(left, right)
    output.append(f"  first-different-byte={offset if offset is not None else 'none'}")
    try:
        fields_a, sections_a, ranges_a = inspect_pe(left)
        fields_b, sections_b, ranges_b = inspect_pe(right)
    except (OSError, ValueError, struct.error) as error:
        output.append(f"  PE parse error: {error}")
        return

    differences = [key for key in sorted(fields_a.keys() | fields_b.keys()) if fields_a.get(key) != fields_b.get(key)]
    if differences:
        output.append("  differing PE fields:")
        for key in differences:
            output.append(f"    {key}: {fields_a.get(key)!r} -> {fields_b.get(key)!r}")
    else:
        output.append("  PE headers: field-identical")
    changed_sections = sorted(sections_a.keys() | sections_b.keys())
    changed_sections = [name for name in changed_sections if sections_a.get(name) != sections_b.get(name)]
    if changed_sections:
        output.append("  differing section content SHA-256:")
        for section in changed_sections:
            output.append(f"    {section}: {sections_a.get(section)!r} -> {sections_b.get(section)!r}")
    elif sections_a:
        output.append("  PE section contents: hash-identical")
    if offset is not None:
        left_data = left.read_bytes()
        right_data = right.read_bytes()
        region_a = next((name for start, end, name in ranges_a if start <= offset < end), "PE headers or overlay")
        region_b = next((name for start, end, name in ranges_b if start <= offset < end), "PE headers or overlay")
        output.append(f"  first-difference-region: build-a={region_a} build-b={region_b}")
        start = max(0, offset - 8)
        end = min(max(len(left_data), len(right_data)), offset + 9)
        output.append(f"  bytes-a[{start}:{end}]={left_data[start:end].hex(' ')}")
        output.append(f"  bytes-b[{start}:{end}]={right_data[start:end].hex(' ')}")


def iter_intermediates(target_root: Path) -> dict[str, Path]:
    extensions = {".obj", ".pdb", ".lib", ".exp"}
    result: dict[str, Path] = {}
    for relative_root in (Path("release/build"), Path("release/deps")):
        root = target_root / relative_root
        if root.is_dir():
            for path in root.rglob("*"):
                if path.is_file() and path.suffix.lower() in extensions:
                    result[path.relative_to(target_root).as_posix()] = path
    return result


def compare_intermediates(build_a: Path, build_b: Path, output: list[str]) -> None:
    files_a, files_b = iter_intermediates(build_a), iter_intermediates(build_b)
    output.append(
        f"MSVC intermediates: build-a={len(files_a)} build-b={len(files_b)} "
        "(release/build and release/deps; .obj/.pdb/.lib/.exp)"
    )
    missing_a, missing_b = sorted(files_b.keys() - files_a.keys()), sorted(files_a.keys() - files_b.keys())
    for label, paths in (("only-build-b", missing_a), ("only-build-a", missing_b)):
        if paths:
            output.append(f"  {label} ({len(paths)}):")
            output.extend(f"    {path}" for path in paths[:40])
            if len(paths) > 40:
                output.append(f"    ... {len(paths) - 40} more")

    differing = [relative for relative in sorted(files_a.keys() & files_b.keys()) if sha256_file(files_a[relative]) != sha256_file(files_b[relative])]
    if not differing:
        output.append("  common intermediate files: byte-identical")
        return
    output.append(f"  differing common files ({len(differing)}):")
    for relative in differing[:100]:
        left, right = files_a[relative], files_b[relative]
        first = first_different_byte(left, right)
        output.append(
            f"    {relative}: size={left.stat().st_size}->{right.stat().st_size} "
            f"first-difference={first if first is not None else 'none'} "
            f"sha256={sha256_file(left)}->{sha256_file(right)}"
        )
    if len(differing) > 100:
        output.append(f"    ... {len(differing) - 100} more")


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print("usage: diagnose-windows-reproducibility.py <build-a> <build-b> <report-file>", file=sys.stderr)
        return 2
    build_a, build_b, report_path = map(Path, argv[1:])
    report: list[str] = ["Windows release reproducibility differential diagnosis"]
    for name in ("synveil-client", "synveil-desktop"):
        compare_pe(build_a, build_b, name, report)
    compare_intermediates(build_a, build_b, report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text("\n".join(report) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
