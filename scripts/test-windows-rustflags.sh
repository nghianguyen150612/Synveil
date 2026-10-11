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

# A clean MSVC-hosted release link must receive deterministic PE timestamp
# policy through both Cargo's encoded environment and the wrapper's direct argv.
msvc_fixture_bin="$fixture_root/msvc-bin"
mkdir -p "$msvc_fixture_bin"
cat > "$msvc_fixture_bin/rustc" <<'SH'
#!/usr/bin/env bash
if [[ "${1:-}" == "-vV" ]]; then
    printf 'host: x86_64-pc-windows-msvc\n'
    exit 0
fi
exit 2
SH
chmod +x "$msvc_fixture_bin/rustc"
cat > "$msvc_fixture_bin/llvm-ar.exe" <<'SH'
#!/usr/bin/env bash
exit 0
SH
chmod +x "$msvc_fixture_bin/llvm-ar.exe"
original_path="$PATH"
PATH="$msvc_fixture_bin:$PATH"
export PATH
QMAKE="$fixture_root/no-qmake"
export QMAKE
MSYSTEM=MINGW64
MSYS2_ENV_CONV_EXCL='SYNVEIL_TEST_SENTINEL'
SYNVEIL_MSVC_AR="$msvc_fixture_bin/llvm-ar.exe"
if command -v cygpath >/dev/null 2>&1; then
    SYNVEIL_MSVC_AR="$(cygpath -w "$SYNVEIL_MSVC_AR")"
fi
AR="$SYNVEIL_MSVC_AR"
export MSYSTEM MSYS2_ENV_CONV_EXCL SYNVEIL_MSVC_AR AR
unset CARGO_ENCODED_RUSTFLAGS RUSTFLAGS
synveil_prepare_reproducible_rust_build "$repo_root"
python3 - <<'PY'
import os

flags = os.environ["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
if flags.count("link-arg=/Brepro") != 1:
    raise SystemExit(f"MSVC reproducible link argument missing or duplicated: {flags!r}")
index = flags.index("link-arg=/Brepro")
if index == 0 or flags[index - 1] != "-C":
    raise SystemExit(f"MSVC reproducible link argument was not passed as a Rust codegen option: {flags!r}")
if os.environ.get("AR") != os.environ.get("SYNVEIL_MSVC_AR"):
    raise SystemExit("MSVC reproducible archiver was not selected by absolute path")
exclusions = os.environ.get("MSYS2_ENV_CONV_EXCL", "").split(";")
for name in ("SYNVEIL_TEST_SENTINEL", "AR", "SYNVEIL_MSVC_AR", "CARGO_ENCODED_RUSTFLAGS"):
    if name not in exclusions:
        raise SystemExit(f"MSYS2_ENV_CONV_EXCL does not preserve {name}")
PY
direct_flags=("${SYNVEIL_REPRODUCIBLE_RUSTC_FLAGS[@]}")
if [[ ! " ${direct_flags[*]} " == *" link-arg=/Brepro "* ]]; then
    echo "MSVC direct rustc wrapper flags omit /Brepro" >&2
    exit 1
fi
cat > "$msvc_fixture_bin/rustc" <<'SH'
#!/usr/bin/env bash
if [[ "$1" == "-vV" ]]; then
    printf 'host: x86_64-pc-windows-msvc\n'
    exit 0
fi
printf '%s\n' "${MSYS2_ARG_CONV_EXCL:-}" > "$SYNVEIL_TEST_EXCLUSION_CAPTURE"
printf '%s\0' "$@" > "$SYNVEIL_TEST_ARG_CAPTURE"
SH
chmod +x "$msvc_fixture_bin/rustc"
SYNVEIL_TEST_EXCLUSION_CAPTURE="$fixture_root/msys-exclusions"
SYNVEIL_TEST_ARG_CAPTURE="$fixture_root/rustc-argv"
export SYNVEIL_TEST_EXCLUSION_CAPTURE SYNVEIL_TEST_ARG_CAPTURE
msys_arg_conv_excl_was_set="${MSYS2_ARG_CONV_EXCL+x}"
msys_arg_conv_excl_before="${MSYS2_ARG_CONV_EXCL:-}"
MSYS2_ARG_CONV_EXCL='--preserve-existing'
export MSYS2_ARG_CONV_EXCL
synveil_run_direct_rustc 'x86_64-pc-windows-msvc' -C 'link-arg=/Brepro'
python3 - <<'PY'
import os
from pathlib import Path

exclusions = Path(os.environ["SYNVEIL_TEST_EXCLUSION_CAPTURE"]).read_text().strip().split(";")
if "--preserve-existing" not in exclusions or "link-arg=/Brepro" not in exclusions:
    raise SystemExit(f"MSYS argument exclusions were not preserved: {exclusions!r}")
argv = Path(os.environ["SYNVEIL_TEST_ARG_CAPTURE"]).read_bytes().split(b"\0")[:-1]
if argv != [b"-C", b"link-arg=/Brepro"]:
    raise SystemExit(f"direct MSVC rustc arguments changed before process launch: {argv!r}")
PY
unset SYNVEIL_TEST_EXCLUSION_CAPTURE SYNVEIL_TEST_ARG_CAPTURE
if [[ -n "$msys_arg_conv_excl_was_set" ]]; then
    MSYS2_ARG_CONV_EXCL="$msys_arg_conv_excl_before"
    export MSYS2_ARG_CONV_EXCL
else
    unset MSYS2_ARG_CONV_EXCL
fi
AR="$msvc_fixture_bin/unselected-ar.exe"
export AR
unset CARGO_ENCODED_RUSTFLAGS RUSTFLAGS
if synveil_prepare_reproducible_rust_build "$repo_root" >"$fixture_root/ar-conflict.log" 2>&1; then
    echo "MSVC reproducible build accepted an unselected AR executable" >&2
    exit 1
fi
if ! grep -Fq "AR conflicts with the selected deterministic MSVC archiver" "$fixture_root/ar-conflict.log"; then
    cat "$fixture_root/ar-conflict.log" >&2
    echo "MSVC reproducible build rejected AR for an unexpected reason" >&2
    exit 1
fi
AR="$SYNVEIL_MSVC_AR"
export AR
PATH="$msvc_fixture_bin:$original_path"
export PATH
unset QMAKE CARGO_ENCODED_RUSTFLAGS RUSTFLAGS

# Exercise explicit and default Windows Cargo homes. On a Windows host, use
# the real cygpath executable so MSYS argument conversion is part of the test.
if command -v cygpath >/dev/null 2>&1; then
    : "${USERPROFILE:?USERPROFILE is required for the Windows Cargo-home test}"
    CARGO_HOME="${USERPROFILE}\\.cargo"
    export CARGO_HOME
    MSYS2_ENV_CONV_EXCL="${MSYS2_ENV_CONV_EXCL:+${MSYS2_ENV_CONV_EXCL};}SYNVEIL_TEST_SENTINEL"
    export MSYS2_ENV_CONV_EXCL
    unset CARGO_ENCODED_RUSTFLAGS RUSTFLAGS
    synveil_prepare_reproducible_rust_build "$repo_root"
    python3 - <<'PY'
import os
import subprocess

cargo_home = os.environ["CARGO_HOME"]
forms = [cargo_home]
forms.extend(subprocess.check_output(["cygpath", mode, cargo_home], text=True).strip() for mode in ("-m", "-w"))
flags = os.environ["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
required = {f"--remap-path-prefix={form}=/usr/local/cargo" for form in forms}
missing = required - set(flags)
if missing:
    raise SystemExit(f"explicit native Cargo home remap missing: {sorted(missing)!r}; flags={flags!r}")
if "CARGO_ENCODED_RUSTFLAGS" not in os.environ.get("MSYS2_ENV_CONV_EXCL", "").split(";"):
    raise SystemExit("MSYS2_ENV_CONV_EXCL does not preserve CARGO_ENCODED_RUSTFLAGS")
if "SYNVEIL_TEST_SENTINEL" not in os.environ.get("MSYS2_ENV_CONV_EXCL", "").split(";"):
    raise SystemExit("MSYS2_ENV_CONV_EXCL overwrote its prior exclusions")
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
if "CARGO_ENCODED_RUSTFLAGS" not in os.environ.get("MSYS2_ENV_CONV_EXCL", "").split(";"):
    raise SystemExit("MSYS2_ENV_CONV_EXCL does not preserve CARGO_ENCODED_RUSTFLAGS")
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
    MSYSTEM=MINGW64
    export MSYSTEM
    USERPROFILE='C:\Users\Test User'
    export USERPROFILE
    CARGO_HOME="$fixture_root/cargo-home/.cargo"
    export CARGO_HOME
    MSYS2_ENV_CONV_EXCL="${MSYS2_ENV_CONV_EXCL:+${MSYS2_ENV_CONV_EXCL};}SYNVEIL_TEST_SENTINEL"
    export MSYS2_ENV_CONV_EXCL
    unset CARGO_ENCODED_RUSTFLAGS RUSTFLAGS
    synveil_prepare_reproducible_rust_build "$repo_root"
    python3 - <<'PY'
import os
import subprocess

cargo_home = os.environ["CARGO_HOME"]
forms = [cargo_home]
forms.extend(subprocess.check_output(["cygpath", mode, cargo_home], text=True).strip() for mode in ("-m", "-w"))
flags = os.environ["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
required = {f"--remap-path-prefix={form}=/usr/local/cargo" for form in forms}
missing = required - set(flags)
if missing:
    raise SystemExit(f"native Cargo home remap missing: {sorted(missing)!r}; flags={flags!r}")
if "CARGO_ENCODED_RUSTFLAGS" not in os.environ.get("MSYS2_ENV_CONV_EXCL", "").split(";"):
    raise SystemExit("MSYS2_ENV_CONV_EXCL does not preserve CARGO_ENCODED_RUSTFLAGS")
if "SYNVEIL_TEST_SENTINEL" not in os.environ.get("MSYS2_ENV_CONV_EXCL", "").split(";"):
    raise SystemExit("MSYS2_ENV_CONV_EXCL overwrote its prior exclusions")
PY

    # Exercise Cargo's default Windows home when CARGO_HOME is unset.
    unset CARGO_HOME CARGO_ENCODED_RUSTFLAGS RUSTFLAGS
    USERPROFILE='C:\Users\Test User'
    export USERPROFILE
    synveil_prepare_reproducible_rust_build "$repo_root"
    python3 - <<'PY'
import os

flags = os.environ["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
required = {
    "--remap-path-prefix=/c/Users/Test User/.cargo=/usr/local/cargo",
    "--remap-path-prefix=C:/Users/Test User/.cargo=/usr/local/cargo",
    r"--remap-path-prefix=C:\Users\Test User\.cargo=/usr/local/cargo",
}
missing = required - set(flags)
if missing:
    raise SystemExit(f"default Windows Cargo home remap missing: {sorted(missing)!r}; flags={flags!r}")
if "CARGO_ENCODED_RUSTFLAGS" not in os.environ.get("MSYS2_ENV_CONV_EXCL", "").split(";"):
    raise SystemExit("MSYS2_ENV_CONV_EXCL does not preserve CARGO_ENCODED_RUSTFLAGS")
PY
fi

unset AR SYNVEIL_MSVC_AR MSYSTEM
echo "windows Rust flag transport: PASS"
