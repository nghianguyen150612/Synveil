#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

PINNED_CBINDGEN_VERSION="0.28.0"
HEADER_PATH="clients/ios/Infrastructure/RustBridge/Generated/synveil_ios_ffi.h"
CONFIG_PATH="crates/ios-ffi/cbindgen.toml"

CHECK_MODE=0

for arg in "$@"; do
    case "${arg}" in
        --check)
            CHECK_MODE=1
            ;;
        *)
            echo "ERROR: Unknown argument: ${arg}" >&2
            echo "Usage: $0 [--check]" >&2
            exit 1
            ;;
    esac
done

ensure_cbindgen() {
    if command -v cbindgen >/dev/null 2>&1; then
        CURRENT_VER="$(cbindgen --version | awk '{print $2}')"
        if [ "${CURRENT_VER}" = "${PINNED_CBINDGEN_VERSION}" ]; then
            return 0
        fi
        echo "Notice: Installed cbindgen version is '${CURRENT_VER}', expected '${PINNED_CBINDGEN_VERSION}'."
    else
        echo "Notice: cbindgen not found in PATH."
    fi

    echo "Installing pinned cbindgen v${PINNED_CBINDGEN_VERSION}..."
    cargo install cbindgen --version "${PINNED_CBINDGEN_VERSION}" --locked || cargo install cbindgen --version "${PINNED_CBINDGEN_VERSION}"
}

ensure_cbindgen

if [ "${CHECK_MODE}" -eq 1 ]; then
    echo "=== Verifying C Header Alignment against Rust Exports (--check) ==="
    TMP_HEADER="$(mktemp /tmp/synveil_ios_ffi.XXXXXX.h)"
    trap 'rm -f "${TMP_HEADER}"' EXIT

    cbindgen --config "${CONFIG_PATH}" --crate synveil-ios-ffi --output "${TMP_HEADER}"

    if ! diff -u "${HEADER_PATH}" "${TMP_HEADER}"; then
        echo "" >&2
        echo "ERROR: Header drift detected in '${HEADER_PATH}'!" >&2
        echo "The committed C header does not match current Rust FFI exports." >&2
        echo "Run './scripts/generate-ios-rust-header.sh' to update the header." >&2
        exit 1
    fi

    echo "SUCCESS: Committed generated header matches current Rust exports."
else
    echo "=== Generating C Header for synveil-ios-ffi ==="
    mkdir -p "$(dirname "${HEADER_PATH}")"
    cbindgen --config "${CONFIG_PATH}" --crate synveil-ios-ffi --output "${HEADER_PATH}"
    echo "SUCCESS: Generated header at '${HEADER_PATH}'"
fi
