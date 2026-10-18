#!/usr/bin/env bash
set -euo pipefail

# Scripts directory baseline
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

echo "=== Synveil iOS Rust Apple Target Verification ==="
echo "Toolchain info:"
rustc --version
rustup --version

echo ""
echo "=== Step 1: Discover & Register Apple Targets ==="
AVAILABLE_TARGETS="$(rustc --print target-list)"

REQUIRED_TARGETS=(
    "aarch64-apple-ios"
    "aarch64-apple-ios-sim"
)

OPTIONAL_TARGETS=(
    "x86_64-apple-ios"
)

TARGETS_TO_BUILD=()

for target in "${REQUIRED_TARGETS[@]}"; do
    if echo "${AVAILABLE_TARGETS}" | grep -q "^${target}$"; then
        echo "Required target found: ${target}"
        TARGETS_TO_BUILD+=("${target}")
    else
        echo "ERROR: Required Apple target '${target}' is not supported by current rustc toolchain." >&2
        exit 1
    fi
done

for target in "${OPTIONAL_TARGETS[@]}"; do
    if echo "${AVAILABLE_TARGETS}" | grep -q "^${target}$"; then
        echo "Optional target found: ${target}"
        TARGETS_TO_BUILD+=("${target}")
    else
        echo "Notice: Optional Apple target '${target}' not present in toolchain target list; skipping."
    fi
done

echo ""
echo "Installing missing targets via rustup..."
for target in "${TARGETS_TO_BUILD[@]}"; do
    rustup target add "${target}"
done

echo ""
echo "=== Step 2: Prohibited Dependency Audit ==="
echo "Auditing dependency closure for synveil-ios-ffi..."
DEP_TREE="$(cargo tree -p synveil-ios-ffi)"

PROHIBITED_CRATES=(
    "synveil-client"
    "synveil-desktop"
    "synveil-install-engine"
    "synveil-api"
    "synveil-metadata"
    "synveil-platform"
    "sqlx"
    "reqwest"
    "tokio"
    "keyring"
    "cxx"
    "cxx-qt"
    "axum"
)

AUDIT_FAILED=0
for crate_name in "${PROHIBITED_CRATES[@]}"; do
    if echo "${DEP_TREE}" | grep -E -q "(^|[^a-zA-Z0-9_-])${crate_name}([^a-zA-Z0-9_-]|$)"; then
        echo "ERROR: Prohibited dependency detected in synveil-ios-ffi closure: ${crate_name}" >&2
        AUDIT_FAILED=1
    fi
done

if [ "${AUDIT_FAILED}" -ne 0 ]; then
    echo "Dependency audit failed." >&2
    exit 1
fi

echo "Dependency audit passed! No prohibited desktop/server/async crates found."

echo ""
echo "=== Step 3: Apple Target Compilation & Build ==="
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-17.0}"
echo "IPHONEOS_DEPLOYMENT_TARGET=${IPHONEOS_DEPLOYMENT_TARGET}"

for target in "${TARGETS_TO_BUILD[@]}"; do
    echo "----------------------------------------"
    echo "Target: ${target}"
    echo "Running cargo check..."
    cargo check -p synveil-ios-ffi --locked --target "${target}"

    echo "Running cargo build..."
    cargo build -p synveil-ios-ffi --locked --target "${target}"

    LIB_PATH="target/${target}/debug/libsynveil_ios_ffi.a"
    if [ -f "${LIB_PATH}" ]; then
        echo "SUCCESS: Produced static library archive at ${LIB_PATH}"
        ls -lh "${LIB_PATH}"
    else
        echo "ERROR: Static library expected at ${LIB_PATH} was not found after build!" >&2
        exit 1
    fi
done

echo ""
echo "=== Step 4: Verify Untracked Binary Output ==="
GIT_UNTRACKED_A="$(git status --short | grep '\.a$' || true)"
if [ -n "${GIT_UNTRACKED_A}" ]; then
    echo "ERROR: Compiled static library archives (.a) detected in git status:" >&2
    echo "${GIT_UNTRACKED_A}" >&2
    exit 1
fi

echo "All Apple Rust targets compiled and verified successfully!"
