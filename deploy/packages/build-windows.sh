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

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd -P)"
# shellcheck source=deploy/packages/common/version.sh
source "${SCRIPT_DIR}/common/version.sh"
# shellcheck source=deploy/packages/common/reproducible.sh
source "${SCRIPT_DIR}/common/reproducible.sh"
# shellcheck source=deploy/packages/common/windows-pe-import-policy.sh
source "${SCRIPT_DIR}/common/windows-pe-import-policy.sh"

OUTPUT_DIR="${REPO_ROOT}/target/windows-packages"
STAGING_DIR=""
CARGO_TARGET_DIR="${REPO_ROOT}/target"
export CARGO_TARGET_DIR
DESKTOP_BINARY=""
CLIENT_BINARY=""
QT_PREFIX="${SYNVEIL_QT_PREFIX:-}"
WINDEPLOYQT="${SYNVEIL_WINDEPLOYQT:-}"
READOBJ="${SYNVEIL_LLVM_READOBJ:-}"

usage() {
    cat >&2 <<'EOF'
Usage: build-windows.sh [--output-dir=DIR] [--desktop-binary=PATH]
                        [--staging-dir=DIR]
                        [--client-binary=PATH] [--qt-prefix=DIR]
                        [--windeployqt=PATH] [--llvm-readobj=PATH]

Build a self-contained, unsigned x86_64 Windows Synveil desktop ZIP.

Options:
  --output-dir=DIR       Output directory (default: target/windows-packages).
                         It must not be / or contain a .. path component.
  --staging-dir=DIR      Export the exact validated runtime closure to DIR for
                         a trusted downstream producer such as the installer.
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
        --staging-dir=*)
            STAGING_DIR="${1#--staging-dir=}"
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

PACKAGE_VERSION="$(synveil_cargo_version)"
SOURCE_DATE_EPOCH="$(synveil_source_date_epoch)"
export SOURCE_DATE_EPOCH
log "archive timestamp: $SOURCE_DATE_EPOCH"
ZIP_PYTHON="${SYNVEIL_PYTHON:-}"
if [[ -z "$ZIP_PYTHON" ]]; then
    ZIP_PYTHON="$(command -v python3 || command -v python || true)"
fi
if [[ -z "$ZIP_PYTHON" ]]; then
    printf '[synveil-windows-package] ERROR: Python 3 is required for deterministic ZIP output\n' >&2
    exit 1
fi
ZIP_TOOLCHAIN="$("$ZIP_PYTHON" -c 'import platform, zlib; print("python-zipfile-" + platform.python_version() + "-zlib-" + zlib.ZLIB_VERSION + "-runtime-" + zlib.ZLIB_RUNTIME_VERSION)')"
log "ZIP writer: $ZIP_TOOLCHAIN"

if [[ -z "$DESKTOP_BINARY" || -z "$CLIENT_BINARY" ]]; then
    synveil_prepare_reproducible_rust_build "$REPO_ROOT"
fi

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
    synveil_assert_portable_release_binary "$binary" "$(basename "$binary")"
done

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
        QtQuick QtQuick/Controls QtQuick/Dialogs QtQuick/Layouts \
        QtQuick/Templates QtQuick/Window; do
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
Plugins=.
QmlImports=qml
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

sha256_file() {
    local file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        printf '[synveil-windows-package] ERROR: SHA-256 tool is required for the package manifest\n' >&2
        return 1
    fi
}

copy_missing_msvc_runtime_imports() {
    if [[ "$native_windows" -ne 1 ]]; then
        return 0
    fi
    if [[ -z "${SYNVEIL_MSVC_CRT_DIR:-}" ]] || ! command -v cygpath >/dev/null 2>&1; then
        printf '[synveil-windows-package] ERROR: native Windows packaging requires the authenticated MSVC CRT directory\n' >&2
        exit 1
    fi
    local crt_dir
    crt_dir="$(cygpath -u "$SYNVEIL_MSVC_CRT_DIR")"
    if [[ ! -d "$crt_dir" || -L "$crt_dir" ]]; then
        printf '[synveil-windows-package] ERROR: authenticated MSVC CRT directory is unavailable\n' >&2
        exit 1
    fi

    # windeployqt --compiler-runtime did not include MSVCP140.dll on the
    # reproduced Windows runner. Add only imported MSVC runtime DLLs from the
    # exact active redist directory; the closed import audit below still
    # rejects every unresolved non-system dependency.
    local changed pe_file imported runtime_file upper
    # The loop counter is intentionally unused; only the bounded pass count matters.
    for _ in 1 2 3 4; do
        changed=0
        while IFS= read -r imported; do
            [[ -n "$imported" ]] || continue
            upper="${imported^^}"
            is_system_dll "$imported" && continue
            [[ -z "$(find_stage_dll "$imported")" ]] || continue
            case "$upper" in
                MSVCP140*.DLL|VCRUNTIME140*.DLL|CONCRT140.DLL|VCCORLIB140.DLL) ;;
                *) continue ;;
            esac
            runtime_file="$(find "$crt_dir" -maxdepth 1 -type f -iname "$imported" -print -quit)"
            if [[ -z "$runtime_file" || -L "$runtime_file" ]]; then
                continue
            fi
            copy_file "$runtime_file" "$STAGE_ROOT/$(basename "$runtime_file")"
            log "added imported MSVC runtime from the active redistributable: $(basename "$runtime_file")"
            changed=$((changed + 1))
        done < <(
            while IFS= read -r pe_file; do
                pe_import_names "$pe_file"
            done < <(find "$STAGE_ROOT" -type f \( -iname '*.exe' -o -iname '*.dll' \) -print | sort) | sort -fu
        )
        [[ "$changed" -gt 0 ]] || break
    done
}

copy_missing_msvc_runtime_imports

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
if find "$STAGE_ROOT" -type d \( -iname include -o -iname mkspecs -o -iname cmake -o -iname pkgconfig -o -iname examples -o -iname tests \) -print -quit | grep -q .; then
    printf '[synveil-windows-package] ERROR: development directory leaked into ZIP\n' >&2
    exit 1
fi
for text_file in "${STAGE_ROOT}/LICENSE" "${STAGE_ROOT}/NOTICE" "${STAGE_ROOT}/qt.conf"; do
    if grep -nE '/(mnt|tmp|home)/|[A-Za-z]:[\\/]Users[\\/].*\\.cargo|/usr/(include|lib)' "$text_file" >/dev/null 2>&1; then
        printf '[synveil-windows-package] ERROR: development path in package metadata: %s\n' "$text_file" >&2
        exit 1
    fi
done

MANIFEST_NAME="SYNVEIL-MANIFEST.txt"
write_package_manifest() {
    local manifest_path="${STAGE_ROOT}/${MANIFEST_NAME}"
    local relative digest size
    {
        printf 'Synveil Windows portable package manifest\n'
        printf 'format=1\n'
        printf 'version=%s\n' "$PACKAGE_VERSION"
        printf 'platform=windows-x86_64\n'
        printf 'source_revision=%s\n' "$(synveil_source_revision "$REPO_ROOT")"
        printf 'source_fingerprint=%s\n' "$(synveil_source_fingerprint "$REPO_ROOT")"
        printf 'source_date_epoch=%s\n' "$SOURCE_DATE_EPOCH"
        printf 'rustc=%s\n' "$(synveil_toolchain_value rustc)"
        printf 'cargo=%s\n' "$(synveil_toolchain_value cargo)"
        printf 'zip_generator=%s\n' "$ZIP_TOOLCHAIN"
        if [[ "$native_windows" -eq 1 ]]; then
            printf 'qt=%s\n' "$(qmake -query QT_VERSION 2>/dev/null || printf unknown)"
            printf 'qt_architecture=x86_64-msvc\n'
            printf 'windeployqt=%s\n' "$($WINDEPLOYQT --version 2>&1 | tr -d '\r' | head -n 1)"
            printf 'msvc=%s\n' "${VCToolsVersion:-unknown}"
            printf 'msvc_crt=VCToolsRedistDir/x64/%s\n' "$(basename "$SYNVEIL_MSVC_CRT_DIR")"
        else
            printf 'qt=%s\n' "$("${QT_PREFIX}/bin/qmake" -query QT_VERSION 2>/dev/null || printf unknown)"
            printf 'qt_architecture=x86_64-cross\n'
            printf 'windeployqt=not-used-cross-build\n'
            printf 'msvc=not-used-cross-build\n'
        fi
        printf 'files=sha256 size path\n'
        while IFS= read -r relative; do
            relative="${relative#./}"
            [[ "$relative" == "$MANIFEST_NAME" ]] && continue
            digest="$(sha256_file "${STAGE_ROOT}/${relative}")"
            size="$(wc -c < "${STAGE_ROOT}/${relative}" | tr -d '[:space:]')"
            printf '%s %s %s\n' "$digest" "$size" "$relative"
        done < <(LC_ALL=C find "$STAGE_ROOT" -type f -printf '%P\n' | LC_ALL=C sort)
    } > "$manifest_path"
}

validate_package_manifest() {
    local manifest_path="${STAGE_ROOT}/${MANIFEST_NAME}"
    local in_files=0
    local entries=0
    local line digest size relative extra actual_hash actual_size
    declare -A seen=()
    grep -Fxq 'Synveil Windows portable package manifest' "$manifest_path"
    grep -Fxq 'format=1' "$manifest_path"
    grep -Fxq "version=${PACKAGE_VERSION}" "$manifest_path"
    grep -Fxq 'platform=windows-x86_64' "$manifest_path"
    while IFS= read -r line || [[ -n "$line" ]]; do
        if [[ "$line" == 'files=sha256 size path' ]]; then
            in_files=1
            continue
        fi
        [[ "$in_files" -eq 1 ]] || continue
        read -r digest size relative extra <<< "$line"
        if [[ -n "$extra" || ! "$digest" =~ ^[0-9a-fA-F]{64}$ || ! "$size" =~ ^[0-9]+$ || -z "$relative" ]]; then
            printf '[synveil-windows-package] ERROR: malformed package manifest entry: %s\n' "$line" >&2
            return 1
        fi
        case "$relative" in
            /*|../*|*/../*|*/..)
                printf '[synveil-windows-package] ERROR: unsafe package manifest path: %s\n' "$relative" >&2
                return 1
                ;;
        esac
        if [[ -n "${seen[$relative]:-}" ]]; then
            printf '[synveil-windows-package] ERROR: duplicate package manifest path: %s\n' "$relative" >&2
            return 1
        fi
        seen["$relative"]=1
        if [[ ! -f "${STAGE_ROOT}/${relative}" ]]; then
            printf '[synveil-windows-package] ERROR: manifest references missing file: %s\n' "$relative" >&2
            return 1
        fi
        actual_hash="$(sha256_file "${STAGE_ROOT}/${relative}")"
        actual_size="$(wc -c < "${STAGE_ROOT}/${relative}" | tr -d '[:space:]')"
        if [[ "$actual_hash" != "${digest,,}" || "$actual_size" != "$size" ]]; then
            printf '[synveil-windows-package] ERROR: package manifest checksum/size mismatch: %s\n' "$relative" >&2
            return 1
        fi
        entries=$((entries + 1))
    done < "$manifest_path"
    [[ "$entries" -gt 0 ]] || {
        printf '[synveil-windows-package] ERROR: package manifest has no file entries\n' >&2
        return 1
    }
    for required in synveil-desktop.exe synveil-client.exe qt.conf platforms/qwindows.dll LICENSE NOTICE; do
        [[ -n "${seen[$required]:-}" ]] || {
            printf '[synveil-windows-package] ERROR: package manifest omits required file: %s\n' "$required" >&2
            return 1
        }
    done
}

write_package_manifest
validate_package_manifest
synveil_assert_no_private_paths "${STAGE_ROOT}/${MANIFEST_NAME}" "$MANIFEST_NAME"
synveil_assert_no_secret_markers "${STAGE_ROOT}/${MANIFEST_NAME}" "$MANIFEST_NAME"
if grep -nE '/(mnt|tmp|home)/|[A-Za-z]:[\\/]Users[\\/].*\\.cargo|/usr/(include|lib)' "${STAGE_ROOT}/${MANIFEST_NAME}" >/dev/null 2>&1; then
    printf '[synveil-windows-package] ERROR: development path in package manifest\n' >&2
    exit 1
fi

# Export only after the closed inventory has been written and validated.  The
# destination is replaced rather than overlaid so stale files cannot enter a
# downstream installer inventory.
if [[ -n "$STAGING_DIR" ]]; then
    if [[ "$STAGING_DIR" != /* ]]; then
        STAGING_DIR="${REPO_ROOT}/${STAGING_DIR}"
    fi
    case "/${STAGING_DIR#/}/" in
        */../*) printf '[synveil-windows-package] ERROR: staging path contains ..: %s\n' "$STAGING_DIR" >&2; exit 1 ;;
    esac
    [[ "$STAGING_DIR" != "/" ]] || { printf '[synveil-windows-package] ERROR: refusing staging directory /\n' >&2; exit 1; }
    rm -rf -- "$STAGING_DIR"
    mkdir -p -- "$STAGING_DIR"
    cp -a "${STAGE_ROOT}/." "$STAGING_DIR/"
    log "validated runtime staging exported: $STAGING_DIR"
fi

ZIP_PATH="${OUTPUT_DIR}/synveil-${PACKAGE_VERSION}-windows-x86_64.zip"
rm -f "$ZIP_PATH"
# Normalize the staging tree for downstream consumers. The ZIP helper also
# writes ordering, timestamps, permissions, and file metadata explicitly.
find "$STAGE_ROOT" -type d -exec chmod 0755 {} +
find "$STAGE_ROOT" -type f -exec chmod 0644 {} +
chmod 0755 "${STAGE_ROOT}/synveil-desktop.exe" "${STAGE_ROOT}/synveil-client.exe"
find "$STAGE_ROOT" -exec touch -d "@${SOURCE_DATE_EPOCH}" {} +
"$ZIP_PYTHON" "${REPO_ROOT}/scripts/create-reproducible-zip.py" \
    --root "$STAGE_ROOT" \
    --output "$ZIP_PATH" \
    --source-date-epoch "$SOURCE_DATE_EPOCH" \
    --executable synveil-desktop.exe \
    --executable synveil-client.exe \
    --required-file synveil-desktop.exe \
    --required-file synveil-client.exe \
    --required-file qt.conf \
    --required-file platforms/qwindows.dll \
    --required-file LICENSE \
    --required-file NOTICE \
    --required-file "$MANIFEST_NAME"

log "ZIP built: $ZIP_PATH"
sha256_file "$ZIP_PATH"
"$ZIP_PYTHON" "${REPO_ROOT}/scripts/release_manifest.py" create \
    --artifact-root "$OUTPUT_DIR" --product-version "$PACKAGE_VERSION" \
    --source-commit "$(git -C "$REPO_ROOT" rev-parse HEAD)" \
    --output "${OUTPUT_DIR}/SYNVEIL-RELEASE-MANIFEST.json" \
    --artifact "{\"id\":\"windows-x86_64-portable\",\"artifact_type\":\"windows_portable_zip\",\"filename\":\"$(basename "$ZIP_PATH")\",\"platform\":\"windows\",\"architecture\":\"x86_64\",\"role\":\"portable\",\"components\":[\"synveil-desktop\",\"synveil-client\"]}"
"$ZIP_PYTHON" "${REPO_ROOT}/scripts/release_manifest.py" validate \
    --artifact-root "$OUTPUT_DIR" "${OUTPUT_DIR}/SYNVEIL-RELEASE-MANIFEST.json"
