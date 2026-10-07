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
fixture_root="$(mktemp -d)"
trap 'rm -f "$capture"; rm -rf -- "$fixture_root"' EXIT
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

# Simulate Git Bash's POSIX view and both native spellings of Cargo's default
# Windows home. This ensures a Windows release build remaps the registry path
# whose backslash form appears in compiled source locations.
mkdir -p "$fixture_root/bin" "$fixture_root/cargo-home/.cargo"
cat > "$fixture_root/bin/cygpath" <<'SH'
#!/usr/bin/env bash
case "$1" in
    -u)
        case "$2" in
            'C:\Users\Test User') printf '%s' '/c/Users/Test User' ;;
            *) printf '%s' "$2" ;;
        esac
        ;;
    -m)
        case "$2" in
            *cargo-home*|*'Users/Test User'*) printf '%s' 'C:/Users/Test User/.cargo' ;;
            *) printf '%s' 'D:/Synveil' ;;
        esac
        ;;
    -w)
        case "$2" in
            *cargo-home*|*'Users/Test User'*) printf '%s' 'C:\Users\Test User\.cargo' ;;
            *) printf '%s' 'D:\Synveil' ;;
        esac
        ;;
    *) exit 2 ;;
esac
SH
chmod +x "$fixture_root/bin/cygpath"
PATH="$fixture_root/bin:$PATH"
export PATH
CARGO_HOME="$fixture_root/cargo-home/.cargo"
export CARGO_HOME
unset CARGO_ENCODED_RUSTFLAGS RUSTFLAGS
synveil_prepare_reproducible_rust_build "$repo_root"
python3 - <<'PY'
import os

flags = os.environ["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
required = {
    "--remap-path-prefix=C:/Users/Test User/.cargo=/usr/local/cargo",
    r"--remap-path-prefix=C:\Users\Test User\.cargo=/usr/local/cargo",
}
missing = required - set(flags)
if missing:
    raise SystemExit(f"native Cargo home remap missing: {sorted(missing)!r}; flags={flags!r}")
PY

# Exercise Cargo's default Windows home when CARGO_HOME is unset, as in hosted
# GitHub Actions. USERPROFILE must win over an unrelated MSYS getent result.
unset CARGO_HOME CARGO_ENCODED_RUSTFLAGS RUSTFLAGS
USERPROFILE='C:\Users\Test User'
export USERPROFILE
synveil_prepare_reproducible_rust_build "$repo_root"
python3 - <<'PY'
import os

flags = os.environ["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
required = r"--remap-path-prefix=C:\Users\Test User\.cargo=/usr/local/cargo"
if required not in flags:
    raise SystemExit(f"default Windows Cargo home remap missing: {required!r}; flags={flags!r}")
PY

echo "windows Rust flag transport: PASS"
