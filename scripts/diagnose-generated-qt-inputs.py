#!/usr/bin/env python3
"""Compare actual nested Cargo Qt/CXX outputs; never alter the byte-equality gate."""
import difflib
import hashlib
from pathlib import Path
import re
import sys

def discover(root):
    files = {}
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(root).as_posix()
        if not ("/out/" in relative or relative.startswith("cxxqt/")):
            continue
        if not (path.suffix in {".cpp", ".cxx", ".h", ".qrc", ".json", ".qmltypes"}
                or path.name == "qmldir" or path.name.endswith(".synveil-qt-original")):
            continue
        key = re.sub(r"(^|/)build/([^/]+)-[0-9a-f]{16}/out/", r"\1build/\2/out/", relative)
        if key in files:
            raise ValueError(f"ambiguous generated input: {key}")
        files[key] = path
    return files

def main():
    left, right, report = map(Path, sys.argv[1:])
    report.mkdir(parents=True, exist_ok=True)
    a, b = discover(left), discover(right)
    lines = [f"Generated Qt/CXX input coverage: A={len(a)}, B={len(b)}\n"]
    if not a or not b:
        lines.append("Generated-input coverage unavailable; an empty report cannot prove deterministic generation.\n")
    for key in sorted(a.keys() | b.keys()):
        if key not in a or key not in b:
            lines.append(f"FILE SET DIFFERS: {key}\n")
            continue
        aa, bb = a[key].read_bytes(), b[key].read_bytes()
        if aa == bb:
            lines.append(f"IDENTICAL: {key} sha256={hashlib.sha256(aa).hexdigest()}\n")
            continue
        lines.append(f"DIFFERS: {key} A={hashlib.sha256(aa).hexdigest()} B={hashlib.sha256(bb).hexdigest()}\n")
        delta = list(difflib.unified_diff(aa.decode(errors="replace").splitlines(True),
                                        bb.decode(errors="replace").splitlines(True),
                                        fromfile="build-a/" + key, tofile="build-b/" + key))
        lines.extend(delta[:80])
        if len(delta) > 80:
            lines.append(f"... {len(delta)-80} further diff lines; full source retained in build trees\n")
    (report / "generated-inputs.txt").write_text("".join(lines))
    print("".join(lines[:12]), end="")

if __name__ == "__main__":
    main()
