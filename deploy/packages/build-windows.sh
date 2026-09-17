#!/usr/bin/env bash
# build-windows.sh — reproducible Windows desktop runtime ZIP.
#
# The ZIP is deliberately not an installer. It contains the two production
# executables, the Qt runtime closure, QML modules/plugins, the C++ runtime,
# and notices. User-level Task Scheduler registration is performed explicitly
# by the running client manager; no service, admin elevation, password, or
# system-wide autostart is packaged here.
#
# On a native Windows runner, windeployqt is the authoritative Qt closure
# resolver. A Linux cross-build may pass --qt-prefix and uses the small,
# explicit QML/runtime allowlist below; it never copies an SDK wholesale.
# SPDX-License-Identifier: MIT
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
# shellcheck source=deploy/packages/common/version.sh
source "${SCRIPT_DIR}/common/version.sh"

OUTPUT_DIR="${REPO_ROOT}/target/windows-packages"
DESKTOP_BINARY=""
CLIENT_BINARY=""
QT_PREFIX="${SYNVEIL_QT_PREFIX:-}"
WINDEPLOYQT="${SYNVEIL_WINDEPLOYQT:-}"
READOBJ="${SYNVEIL_LLVM_READOBJ:-}"

usage() {
    cat >&2 <<'EOF'
Usage: build-windows.sh [--output-dir=DIR] [--desktop-binary=PATH]
                        [--client-binary=PATH] [--qt-prefix=DIR]
                        [--windeployqt=PATH] [--llvm-readobj=PATH]

Build a self-contained, unsigned x86_64 Windows Synveil desktop ZIP.

Options:
  --output-dir=DIR       Output directory (default: target/windows-packages).
                         It must not be / or contain a .. path component.
  --desktop-binary=PATH  Existing synveil-desktop.exe. If omitted, build the
                         release native Windows Qt target in the current tree.
  --client-binary=PATH   Existing sibling synveil-client.exe. If omitted,
                         build the release native Windows client target.
  --qt-prefix=DIR        Windows Qt prefix for a Linux cross-build fallback.
                         On a native Windows runner windeployqt discovers it.
  --windeployqt=PATH     Explicit windeployqt executable.
  --llvm-readobj=PATH    Explicit llvm-readobj used for PE import closure.
  --help                 Show help.

Native Windows builds require windeployqt and either llvm-readobj or dumpbin.
Linux cross-builds require --qt-prefix and a real cross-target desktop/client
pair. The fallback copies only runtime DLLs, qwindows.dll, and the QML modules
used by the embedded shell; it never packages headers, import libraries, or a
whole Qt SDK.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --output-dir=*)
            OUTPUT_DIR="${1#--output-dir=}"
            shift
            ;;
        --desktop-binary=*)
            DESKTOP_BINARY="${1#--desktop-binary=}"
            shift
            ;;
        --client-binary=*)
            CLIENT_BINARY="${1#--client-binary=}"
            shift
            ;;
        --qt-prefix=*)
            QT_PREFIX="${1#--qt-prefix=}"
            shift
            ;;
        --windeployqt=*)
            WINDEPLOYQT="${1#--windeployqt=}"
            shift
            ;;
        --llvm-readobj=*)
            READOBJ="${1#--llvm-readobj=}"
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        --)
            shift
            break
            ;;
        -*|*)
            printf '[synveil-windows-package] ERROR: unexpected argument: %s\n' "$1" >&2
            usage
            exit 2
            ;;
    esac
done

if [[ "$OUTPUT_DIR" != /* ]]; then
    OUTPUT_DIR="${REPO_ROOT}/${OUTPUT_DIR}"
fi
if [[ "$OUTPUT_DIR" == "/" ]]; then
    printf '[synveil-windows-package] ERROR: refusing output directory /\n' >&2
    exit 1
fi
if [[ "$OUTPUT_DIR" == *".."* ]]; then
    IFS='/' read -ra output_parts <<< "$OUTPUT_DIR"
    for part in "${output_parts[@]}"; do
        if [[ "$part" == ".." ]]; then
            printf '[synveil-windows-package] ERROR: output path contains ..: %s\n' "$OUTPUT_DIR" >&2
            exit 1
        fi
    done
fi
mkdir -p "$OUTPUT_DIR"

log() {
    printf '[synveil-windows-package] %s\n' "$*" >&2
}

find_existing_binary() {
    local requested="$1"
    local name="$2"
    if [[ -n "$requested" ]]; then
        printf '%s' "$requested"
        return 0
    fi
    for candidate in \
        "${REPO_ROOT}/target/x86_64-pc-windows-gnu/release/${name}.exe" \
        "${REPO_ROOT}/target/release/${name}.exe" \
        "${REPO_ROOT}/target/release/${name}"; do
        if [[ -f "$candidate" ]]; then
            printf '%s' "$candidate"
            return 0
        fi
    done
    return 1
}

if [[ -z "$DESKTOP_BINARY" ]]; then
    if DESKTOP_BINARY="$(find_existing_binary "" synveil-desktop)"; then
        :
    else
        log "building release Windows desktop target"
        (cd "$REPO_ROOT" && cargo build --release --locked -p synveil-desktop --target x86_64-pc-windows-gnu)
        DESKTOP_BINARY="${REPO_ROOT}/target/x86_64-pc-windows-gnu/release/synveil-desktop.exe"
    fi
fi
if [[ -z "$CLIENT_BINARY" ]]; then
    if CLIENT_BINARY="$(find_existing_binary "" synveil-client)"; then
        :
    else
        log "building release Windows client target"
        (cd "$REPO_ROOT" && cargo build --release --locked -p synveil-client --target x86_64-pc-windows-gnu)
        CLIENT_BINARY="${REPO_ROOT}/target/x86_64-pc-windows-gnu/release/synveil-client.exe"
    fi
fi

for binary in "$DESKTOP_BINARY" "$CLIENT_BINARY"; do
    if [[ ! -f "$binary" ]]; then
        printf '[synveil-windows-package] ERROR: executable does not exist: %s\n' "$binary" >&2
        exit 1
    fi
    if [[ "$binary" == *".."* ]]; then
        IFS='/' read -ra binary_parts <<< "$binary"
        for part in "${binary_parts[@]}"; do
            if [[ "$part" == ".." ]]; then
                printf '[synveil-windows-package] ERROR: executable path contains ..: %s\n' "$binary" >&2
                exit 1
            fi
        done
    fi
done

PACKAGE_VERSION="$(synveil_cargo_version)"
STAGE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/synveil-windows-stage.XXXXXX")"
trap 'rm -rf -- "$STAGE_ROOT"' EXIT

copy_file() {
    local source="$1"
    local destination="$2"
    if [[ ! -f "$source" ]]; then
        printf '[synveil-windows-package] ERROR: required runtime file missing: %s\n' "$source" >&2
        exit 1
    fi
    mkdir -p "$(dirname "$destination")"
    cp -a "$source" "$destination"
}

copy_optional_file() {
    local source="$1"
    local destination="$2"
    if [[ -f "$source" ]]; then
        mkdir -p "$(dirname "$destination")"
        cp -a "$source" "$destination"
    fi
}

copy_file "$DESKTOP_BINARY" "${STAGE_ROOT}/synveil-desktop.exe"
copy_file "$CLIENT_BINARY" "${STAGE_ROOT}/synveil-client.exe"
copy_file "${REPO_ROOT}/LICENSE" "${STAGE_ROOT}/LICENSE"
copy_file "${REPO_ROOT}/deploy/NOTICE" "${STAGE_ROOT}/NOTICE"

native_windows=0
case "$(uname -s 2>/dev/null || true)" in
    MINGW*|MSYS*|CYGWIN*) native_windows=1 ;;
esac

if [[ -z "$WINDEPLOYQT" ]]; then
    if command -v windeployqt >/dev/null 2>&1; then
        WINDEPLOYQT="$(command -v windeployqt)"
    elif command -v windeployqt6 >/dev/null 2>&1; then
        WINDEPLOYQT="$(command -v windeployqt6)"
    fi
fi

if [[ "$native_windows" -eq 1 ]]; then
    if [[ -z "$WINDEPLOYQT" || ! -f "$WINDEPLOYQT" ]]; then
        printf '[synveil-windows-package] ERROR: native Windows packaging requires windeployqt\n' >&2
        exit 1
    fi
    log "deploying the Qt closure with windeployqt"
    "$WINDEPLOYQT" \
        --release \
        --compiler-runtime \
        --no-translations \
        --no-system-d3d-compiler \
        --qmldir "${REPO_ROOT}/crates/desktop/qml" \
        "${STAGE_ROOT}/synveil-desktop.exe"
else
    if [[ -z "$QT_PREFIX" || ! -d "$QT_PREFIX" ]]; then
        printf '[synveil-windows-package] ERROR: Linux cross packaging requires --qt-prefix=DIR\n' >&2
        exit 1
    fi
    qt_bin="${QT_PREFIX}/bin"
    qt_qml="${QT_PREFIX}/qml"
    log "deploying the explicit cross-target Qt runtime closure from $QT_PREFIX"
    for dll in \
        Qt6Core.dll Qt6Gui.dll Qt6Widgets.dll Qt6Network.dll Qt6Qml.dll \
        Qt6QmlCore.dll Qt6QmlLocalStorage.dll Qt6QmlMeta.dll Qt6QmlNetwork.dll \
        Qt6QmlModels.dll Qt6QmlWorkerScript.dll Qt6QmlXmlListModel.dll \
        Qt6Quick.dll Qt6OpenGL.dll Qt6QuickControls2.dll \
        Qt6QuickControls2Basic.dll Qt6QuickControls2BasicStyleImpl.dll \
        Qt6QuickControls2FluentWinUI3StyleImpl.dll Qt6QuickControls2Fusion.dll \
        Qt6QuickControls2FusionStyleImpl.dll Qt6QuickControls2Imagine.dll \
        Qt6QuickControls2ImagineStyleImpl.dll Qt6QuickControls2Material.dll \
        Qt6QuickControls2MaterialStyleImpl.dll Qt6QuickControls2Universal.dll \
        Qt6QuickControls2UniversalStyleImpl.dll Qt6QuickControls2WindowsStyleImpl.dll \
        Qt6QuickControls2Impl.dll Qt6QuickDialogs2.dll \
        Qt6QuickDialogs2QuickImpl.dll Qt6QuickDialogs2Utils.dll Qt6QuickEffects.dll \
        Qt6QuickLayouts.dll Qt6QuickParticles.dll Qt6QuickShapes.dll \
        Qt6QuickTemplates2.dll Qt6QuickVectorImage.dll Qt6QuickVectorImageGenerator.dll \
        Qt6Sql.dll Qt6Svg.dll; do
        copy_file "${qt_bin}/${dll}" "${STAGE_ROOT}/${dll}"
    done
    copy_file "${QT_PREFIX}/plugins/platforms/qwindows.dll" "${STAGE_ROOT}/platforms/qwindows.dll"
    # These are the only QML imports used by the embedded Main.qml and their
    # direct Qt module dependencies. Copying a module includes its qml files,
    # qmldir metadata, and plugin DLLs, never headers/import libraries.
    for module in \
        QtCore QtNetwork QtQml QtQml/Models QtQml/WorkerScript \
        QtQuick QtQuick/Controls QtQuick/Layouts QtQuick/Templates QtQuick/Window; do
        if [[ ! -d "${qt_qml}/${module}" ]]; then
            printf '[synveil-windows-package] ERROR: required QML module missing: %s\n' "${module}" >&2
            exit 1
        fi
        mkdir -p "${STAGE_ROOT}/qml/$(dirname "$module")"
        cp -a "${qt_qml}/${module}" "${STAGE_ROOT}/qml/$(dirname "$module")/"
    done
    for runtime_dll in libc++.dll libstdc++-6.dll libunwind.dll libwinpthread-1.dll libgcc_s_seh-1.dll; do
        copy_optional_file "${qt_bin}/${runtime_dll}" "${STAGE_ROOT}/${runtime_dll}"
    done
fi

# The executable is beside the runtime; make Qt's plugin/QML roots explicit so
# the ZIP remains independent of a machine-wide Qt installation.
cat > "${STAGE_ROOT}/qt.conf" <<'EOF'
[Paths]
Prefix=.
Plugins=plugins
Qml2Imports=qml
EOF

assert_pe() {
    local file="$1"
    local output=""
    if [[ -z "$READOBJ" ]]; then
        if command -v llvm-readobj >/dev/null 2>&1; then
            READOBJ="$(command -v llvm-readobj)"
        elif command -v llvm-readobj.exe >/dev/null 2>&1; then
            READOBJ="$(command -v llvm-readobj.exe)"
        fi
    fi
    if [[ -n "$READOBJ" && -f "$READOBJ" ]]; then
        output="$("$READOBJ" --file-headers "$file")"
        grep -q "IMAGE_FILE_MACHINE_AMD64" <<< "$output" || {
            printf '[synveil-windows-package] ERROR: non-AMD64 PE: %s\n' "$file" >&2
            exit 1
        }
    elif command -v file >/dev/null 2>&1; then
        file -b "$file" | grep -Eq 'PE32\+.*x86-64|PE32\+.*AMD64' || {
            printf '[synveil-windows-package] ERROR: not an x86_64 PE: %s\n' "$file" >&2
            exit 1
        }
    else
        printf '[synveil-windows-package] ERROR: PE audit requires llvm-readobj or file\n' >&2
        exit 1
    fi
}

for executable in "${STAGE_ROOT}/synveil-desktop.exe" "${STAGE_ROOT}/synveil-client.exe"; do
    assert_pe "$executable"
done

is_system_dll() {
    local name="${1^^}"
    case "$name" in
        API-MS-WIN-*|EXT-MS-WIN-*|KERNEL32.DLL|KERNELBASE.DLL|NTDLL.DLL|ADVAPI32.DLL|\
        USER32.DLL|GDI32.DLL|OLE32.DLL|OLEAUT32.DLL|SHELL32.DLL|SHLWAPI.DLL|COMDLG32.DLL|\
        COMBASE.DLL|WS2_32.DLL|IPHLPAPI.DLL|CRYPT32.DLL|BCRYPT.DLL|BCRYPTPRIMITIVES.DLL|WINHTTP.DLL|\
        VERSION.DLL|DWMAPI.DLL|IMM32.DLL|SETUPAPI.DLL|AUTHZ.DLL|D3D11.DLL|D3D12.DLL|\
        D3D9.DLL|DNSAPI.DLL|DWRITE.DLL|DXGI.DLL|IMAGEHLP.DLL|MPR.DLL|MSVCRT.DLL|\
        NETAPI32.DLL|RPCRT4.DLL|SECUR32.DLL|SHCORE.DLL|USERENV.DLL|UXTHEME.DLL|\
        WINMM.DLL|WINSPOOL.DRV|WTSAPI32.DLL)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

pe_import_names() {
    local file="$1"
    if [[ -n "$READOBJ" && -f "$READOBJ" ]]; then
        "$READOBJ" --coff-imports "$file" |
            sed -n 's/^[[:space:]]*Name:[[:space:]]*//p' |
            tr -d '\r' | sed '/^$/d' | sort -fu
        return 0
    fi
    if command -v dumpbin >/dev/null 2>&1; then
        dumpbin /dependents "$file" 2>/dev/null |
            sed -nE 's/^[[:space:]]*([A-Za-z0-9_.-]+\.dll)[[:space:]]*$/\1/ip' |
            sort -fu
        return 0
    fi
    printf '[synveil-windows-package] ERROR: import audit requires llvm-readobj or dumpbin\n' >&2
    exit 1
}

find_stage_dll() {
    local requested="${1,,}"
    find "$STAGE_ROOT" -type f -iname "$requested" -print -quit
}

# Audit every shipped PE's imports. Windows system/API-set DLLs are supplied by
# Windows; every other imported DLL must be in this ZIP. This catches Linux
# shared-library leakage and incomplete Qt/C++ runtime closure.
while IFS= read -r pe_file; do
    while IFS= read -r imported; do
        [[ -z "$imported" ]] && continue
        if is_system_dll "$imported"; then
            continue
        fi
        if [[ -z "$(find_stage_dll "$imported")" ]]; then
            printf '[synveil-windows-package] ERROR: missing non-system import %s required by %s\n' "$imported" "$pe_file" >&2
            exit 1
        fi
    done < <(pe_import_names "$pe_file")
done < <(find "$STAGE_ROOT" -type f \( -iname '*.exe' -o -iname '*.dll' \) -print | sort)

# Reject development/SDK material and source/build paths from the archive.
if find "$STAGE_ROOT" -type f \( -name '*.a' -o -name '*.lib' -o -name '*.prl' -o -name '*.so' -o -name '*.h' \) -print -quit | grep -q .; then
    printf '[synveil-windows-package] ERROR: development artifact leaked into ZIP\n' >&2
    exit 1
fi
for text_file in "${STAGE_ROOT}/LICENSE" "${STAGE_ROOT}/NOTICE" "${STAGE_ROOT}/qt.conf"; do
    if grep -nE '/(mnt|tmp|home)/|[A-Za-z]:[\\/]Users[\\/].*\\.cargo|/usr/(include|lib)' "$text_file" >/dev/null 2>&1; then
        printf '[synveil-windows-package] ERROR: development path in package metadata: %s\n' "$text_file" >&2
        exit 1
    fi
done

if ! command -v zip >/dev/null 2>&1; then
    printf '[synveil-windows-package] ERROR: zip is required for reproducible ZIP output\n' >&2
    exit 1
fi

ZIP_PATH="${OUTPUT_DIR}/synveil-${PACKAGE_VERSION}-windows-x86_64.zip"
rm -f "$ZIP_PATH"
if [[ -n "${SOURCE_DATE_EPOCH:-}" ]]; then
    find "$STAGE_ROOT" -type f -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +
fi
(
    cd "$STAGE_ROOT"
    LC_ALL=C find . -type f -print | sort | zip -X -q "$ZIP_PATH" -@
)

# Final archive-level path audit, independent of the staging tree.
if command -v unzip >/dev/null 2>&1; then
    archive_files="$(unzip -Z1 "$ZIP_PATH" | LC_ALL=C sort)"
    grep -Fxq 'synveil-desktop.exe' <<< "$archive_files"
    grep -Fxq 'synveil-client.exe' <<< "$archive_files"
    grep -Fxq 'qt.conf' <<< "$archive_files"
    grep -Fxq 'platforms/qwindows.dll' <<< "$archive_files"
    grep -Fxq 'LICENSE' <<< "$archive_files"
    grep -Fxq 'NOTICE' <<< "$archive_files"
    if grep -Eq '(^|/)(include|lib|Headers|cmake)(/|$)|\.(a|lib|prl|so)$' <<< "$archive_files"; then
        printf '[synveil-windows-package] ERROR: archive contains SDK/development path\n' >&2
        exit 1
    fi
fi

log "ZIP built: $ZIP_PATH"
sha256sum "$ZIP_PATH" 2>/dev/null || shasum -a 256 "$ZIP_PATH"
