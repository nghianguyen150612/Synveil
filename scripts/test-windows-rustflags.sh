#!/usr/bin/env bash
# Regression test for Cargo and direct-rustc argument transport from Git Bash.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
# shellcheck source=deploy/packages/common/reproducible.sh
source "$repo_root/deploy/packages/common/reproducible.sh"

flags=(
    '--remap-path-prefix=C:\Users\Test User\.cargo=/usr/local/cargo'
    '--remap-path-prefix=C:\Program Files\Something=/usr/local/something'
    '--remap-path-prefix=D:\A\Synveil=/usr/src/synveil'
)
synveil_export_encoded_rustflags "${flags[@]}"

python3 - "${flags[@]}" <<'PY'
import os
import sys

expected = sys.argv[1:]
actual = os.environ["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
if actual != expected:
    raise SystemExit(f"encoded Cargo flags changed: {actual!r}")
if "RUSTFLAGS" in os.environ:
    raise SystemExit("ambiguous plain RUSTFLAGS remained exported")
PY

# The Qt wrapper's direct rustc call consumes the same array, not an encoded or
# whitespace-delimited shell string. Check the argv boundary with a stand-in.
capture="$(mktemp)"
trap 'rm -f "$capture"' EXIT
capture_argv() { printf '%s\0' "$@" > "$capture"; }
SYNVEIL_REPRODUCIBLE_RUSTC_FLAGS=("${flags[@]}")
capture_argv "${SYNVEIL_REPRODUCIBLE_RUSTC_FLAGS[@]}" --edition=2021 wrapper.rs -o wrapper.exe
python3 - "$capture" "${flags[@]}" <<'PY'
from pathlib import Path
import sys

actual = Path(sys.argv[1]).read_bytes().split(b"\0")[:-1]
expected = [value.encode() for value in sys.argv[2:]]
if actual[:len(expected)] != expected:
    raise SystemExit(f"direct rustc flags changed: {actual!r}")
PY

echo "windows Rust flag transport: PASS"
