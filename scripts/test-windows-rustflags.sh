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

# Exercise explicit and default Windows Cargo homes. On a Windows host, use
# the real cygpath executable so MSYS argument conversion is part of the test.
if command -v cygpath >/dev/null 2>&1; then
    : "${USERPROFILE:?USERPROFILE is required for the Windows Cargo-home test}"
    CARGO_HOME="${USERPROFILE}\\.cargo"
    export CARGO_HOME
    unset CARGO_ENCODED_RUSTFLAGS RUSTFLAGS
    synveil_prepare_reproducible_rust_build "$repo_root"
    python3 - <<'PY'
import os
import subprocess

profile = os.environ["USERPROFILE"]
posix_profile = subprocess.check_output(["cygpath", "-u", profile], text=True).strip()
cargo_home = posix_profile.rstrip("/\\") + "/.cargo"
forms = [cargo_home]
forms.extend(subprocess.check_output(["cygpath", mode, cargo_home], text=True).strip() for mode in ("-m", "-w"))
flags = os.environ["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
required = {f"--remap-path-prefix={form}=/usr/local/cargo" for form in forms}
missing = required - set(flags)
if missing:
    raise SystemExit(f"explicit native Cargo home remap missing: {sorted(missing)!r}; flags={flags!r}")
PY

    # Cargo defaults to USERPROFILE\\.cargo when CARGO_HOME is absent.
    unset CARGO_HOME CARGO_ENCODED_RUSTFLAGS RUSTFLAGS
    synveil_prepare_reproducible_rust_build "$repo_root"
    python3 - <<'PY'
import os
import subprocess

profile = os.environ["USERPROFILE"]
posix_profile = subprocess.check_output(["cygpath", "-u", profile], text=True).strip()
cargo_home = posix_profile.rstrip("/\\") + "/.cargo"
forms = [cargo_home]
forms.extend(subprocess.check_output(["cygpath", mode, cargo_home], text=True).strip() for mode in ("-m", "-w"))
flags = os.environ["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
required = {f"--remap-path-prefix={form}=/usr/local/cargo" for form in forms}
missing = required - set(flags)
if missing:
    raise SystemExit(f"default native Cargo home remap missing: {sorted(missing)!r}; flags={flags!r}")
PY
else
    # Simulate native Windows spellings on hosts without Git Bash/cygpath.
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

    # Exercise Cargo's default Windows home when CARGO_HOME is unset.
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
fi

echo "windows Rust flag transport: PASS"
