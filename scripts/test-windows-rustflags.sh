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
probe_target_dir=""
cleanup() {
    rm -f "$capture"
    rm -rf -- "$fixture_root"
    if [[ -n "$probe_target_dir" ]]; then
        rm -rf -- "$probe_target_dir"
    fi
}
trap cleanup EXIT
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
# Keep Qt wrapper discovery disabled for every simulated Cargo-home case below.
# Windows installer validation runs this test after installing Qt, while the
# Rust CI job runs it before Qt is installed.
MSYSTEM=MINGW64
MSYS2_ENV_CONV_EXCL='SYNVEIL_TEST_SENTINEL'
SYNVEIL_MSVC_AR="$msvc_fixture_bin/llvm-ar.exe"
if command -v cygpath >/dev/null 2>&1; then
    SYNVEIL_MSVC_AR="$(cygpath -w "$SYNVEIL_MSVC_AR")"
fi
AR="$SYNVEIL_MSVC_AR"
CFLAGS='/DKEEP_C_FLAG'
CXXFLAGS='/DKEEP_CXX_FLAG'
export MSYSTEM MSYS2_ENV_CONV_EXCL SYNVEIL_MSVC_AR AR
export CFLAGS CXXFLAGS
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
cxx_flags = os.environ.get("CXXFLAGS", "").split()
if (cxx_flags.count("/Brepro") != 1 or
        cxx_flags.count("/experimental:deterministic") != 1 or
        "/DKEEP_CXX_FLAG" not in cxx_flags):
    raise SystemExit(f"MSVC deterministic C++ flag was missing, duplicated, or replaced caller flags: {cxx_flags!r}")
c_flags = os.environ.get("CFLAGS", "").split()
if (c_flags.count("/Brepro") != 1 or
        c_flags.count("/experimental:deterministic") != 1 or
        "/DKEEP_C_FLAG" not in c_flags):
    raise SystemExit(f"MSVC deterministic C flag was missing, duplicated, or replaced caller flags: {c_flags!r}")
exclusions = os.environ.get("MSYS2_ENV_CONV_EXCL", "").split(";")
for name in ("SYNVEIL_TEST_SENTINEL", "AR", "SYNVEIL_MSVC_AR", "CFLAGS", "CXXFLAGS", "CARGO_ENCODED_RUSTFLAGS"):
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
unset CARGO_ENCODED_RUSTFLAGS RUSTFLAGS

# Exercise explicit and default Windows Cargo homes. On a Windows host, use
# the real cygpath executable so MSYS argument conversion is part of the test.
if command -v cygpath >/dev/null 2>&1 &&
   [[ -n "${VCToolsInstallDir:-}" ]] &&
   [[ "${OSTYPE:-}" == msys* || "${OSTYPE:-}" == cygwin* ]]; then
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
            "$SYNVEIL_TEST_REPO_ROOT") printf '%s' 'D:/Synveil' ;;
            "$SYNVEIL_TEST_REPO_ROOT"/target*)
                printf 'D:/Synveil/target%s' "${2#"$SYNVEIL_TEST_REPO_ROOT"/target}"
                ;;
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
    SYNVEIL_TEST_REPO_ROOT="$repo_root"
    export SYNVEIL_TEST_REPO_ROOT
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
for name in ("CFLAGS", "CXXFLAGS"):
    flags = os.environ.get(name, "").split()
    expected = {
        "/Brepro",
        "/experimental:deterministic",
        "/pathmap:D:/Synveil=/usr/src/synveil",
        "/pathmap:D:/Synveil/target=/usr/src/synveil-target",
    }
    missing = expected - set(flags)
    if missing:
        raise SystemExit(f"{name} missing deterministic MSVC compiler flags: {sorted(missing)!r}; flags={flags!r}")
    target_map = flags.index("/pathmap:D:/Synveil/target=/usr/src/synveil-target")
    checkout_map = flags.index("/pathmap:D:/Synveil=/usr/src/synveil")
    if target_map > checkout_map:
        raise SystemExit(f"{name} maps the broad checkout before its nested target root: {flags!r}")
    if flags.count("/Brepro") != 1 or flags.count("/experimental:deterministic") != 1:
        raise SystemExit(f"{name} duplicates deterministic MSVC compiler flags: {flags!r}")
    if name not in os.environ.get("MSYS2_ENV_CONV_EXCL", "").split(";"):
        raise SystemExit(f"MSYS2_ENV_CONV_EXCL does not preserve {name}")
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

if [[ -n "${VCToolsInstallDir:-}" ]] &&
   [[ "${OSTYPE:-}" == msys* || "${OSTYPE:-}" == cygwin* ]]; then
    # Compile a real C++ translation unit with the compiler under the active
    # Visual Studio toolset. Its included header uses CXX-Qt's generated-header
    # layout below a randomized target root nested inside the checkout. The
    # object must use the stable target prefix and omit that randomized root.
    : "${VCToolsInstallDir:?VCToolsInstallDir is required for the MSVC path-map probe}"
    native_msvc_tools_root="$(cygpath -u "$VCToolsInstallDir")"
    native_msvc_cl="${native_msvc_tools_root%/}/bin/Hostx64/x64/cl.exe"
    if [[ ! -x "$native_msvc_cl" ]]; then
        echo "active MSVC compiler is missing from VCToolsInstallDir: $native_msvc_cl" >&2
        exit 1
    fi
    mkdir -p "$repo_root/target"
    probe_target_dir="$(mktemp -d "$repo_root/target/synveil-repro-msvc-pathmap.XXXXXX")"
    probe_header_dir="$probe_target_dir/release/build/synveil-desktop-pathmap/out/cxxqtbuild/include/cxx-qt"
    probe_source="$fixture_root/pathmap-probe.cpp"
    probe_object="$fixture_root/pathmap-probe.obj"
    mkdir -p "$probe_header_dir"
    cat > "$probe_header_dir/thread.h" <<'CPP'
inline const char* synveil_pathmap_probe_header() { return __FILE__; }
CPP
    cat > "$probe_source" <<'CPP'
#include <cxx-qt/thread.h>
int main() { return synveil_pathmap_probe_header()[0] == '\0'; }
CPP

    probe_original_target_dir_set="${CARGO_TARGET_DIR+x}"
    probe_original_target_dir="${CARGO_TARGET_DIR:-}"
    CARGO_TARGET_DIR="$probe_target_dir"
    CFLAGS='/DKEEP_C_FLAG'
    CXXFLAGS='/DKEEP_CXX_FLAG'
    export CARGO_TARGET_DIR CFLAGS CXXFLAGS
    unset CARGO_ENCODED_RUSTFLAGS RUSTFLAGS
    synveil_prepare_reproducible_rust_build "$repo_root"
    PROBE_TARGET_MAP_PREFIX="$(cygpath -m "$probe_target_dir")"
    PROBE_REPO_MAP_PREFIX="$(cygpath -m "$repo_root")"
    export PROBE_TARGET_MAP_PREFIX PROBE_REPO_MAP_PREFIX
    python3 - <<'PY'
import os

flags = os.environ["CXXFLAGS"].split()
target_map = f"/pathmap:{os.environ['PROBE_TARGET_MAP_PREFIX']}=/usr/src/synveil-target"
checkout_map = f"/pathmap:{os.environ['PROBE_REPO_MAP_PREFIX']}=/usr/src/synveil"
if target_map not in flags or checkout_map not in flags:
    raise SystemExit(f"MSVC CXXFLAGS omit required path maps: {flags!r}")
if flags.index(target_map) > flags.index(checkout_map):
    raise SystemExit(f"MSVC CXXFLAGS map the checkout before its nested target root: {flags!r}")
PY
    read -r -a probe_cxx_flags <<< "$CXXFLAGS"
    probe_header_root_windows="$(cygpath -w "$probe_target_dir/release/build/synveil-desktop-pathmap/out/cxxqtbuild/include")"
    probe_source_windows="$(cygpath -w "$probe_source")"
    probe_object_windows="$(cygpath -w "$probe_object")"
    probe_arg_conv_excl_was_set="${MSYS2_ARG_CONV_EXCL+x}"
    probe_arg_conv_excl_before="${MSYS2_ARG_CONV_EXCL:-}"
    MSYS2_ARG_CONV_EXCL='*'
    export MSYS2_ARG_CONV_EXCL
    # Keep debug metadata disabled so any target-root path found in the object
    # comes from the included generated header's __FILE__ string.
    "$native_msvc_cl" /nologo /FC /Od /c "${probe_cxx_flags[@]}" /I "$probe_header_root_windows" "$probe_source_windows" "/Fo$probe_object_windows"
    if [[ -n "$probe_arg_conv_excl_was_set" ]]; then
        MSYS2_ARG_CONV_EXCL="$probe_arg_conv_excl_before"
        export MSYS2_ARG_CONV_EXCL
    else
        unset MSYS2_ARG_CONV_EXCL
    fi
    PROBE_OBJECT="$probe_object" python3 - <<'PY'
import os
from pathlib import Path

data = Path(os.environ["PROBE_OBJECT"]).read_bytes()
normalized = data.replace(b"\\", b"/").lower()
expected = b"/usr/src/synveil-target/release/build/synveil-desktop-pathmap/out/cxxqtbuild/include/cxx-qt/thread.h"
if expected not in normalized:
    excerpts = [part.decode("ascii", "replace") for part in data.split(b"\0") if b"thread.h" in part.lower()]
    raise SystemExit(f"MSVC object omitted the stable nested target path {expected!r}; path strings={excerpts!r}")
if b"synveil-repro-msvc-pathmap." in normalized:
    raise SystemExit("MSVC object retained the randomized CARGO_TARGET_DIR path")
PY
    if [[ -n "$probe_original_target_dir_set" ]]; then
        CARGO_TARGET_DIR="$probe_original_target_dir"
        export CARGO_TARGET_DIR
    else
        unset CARGO_TARGET_DIR
    fi
fi

unset AR SYNVEIL_MSVC_AR MSYSTEM QMAKE
echo "windows Rust flag transport: PASS"
