#!/usr/bin/env bash
set -euo pipefail

# Scripts directory baseline
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

echo "=== Synveil iOS Rust Apple Artifact Pipeline ==="
echo "Toolchain info:"
rustc --version
rustup --version
if command -v lipo >/dev/null 2>&1; then
    lipo -version || true
fi

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

X86_64_SIM_AVAILABLE=0
for target in "${OPTIONAL_TARGETS[@]}"; do
    if echo "${AVAILABLE_TARGETS}" | grep -q "^${target}$"; then
        echo "Optional target found: ${target}"
        TARGETS_TO_BUILD+=("${target}")
        if [ "${target}" = "x86_64-apple-ios" ]; then
            X86_64_SIM_AVAILABLE=1
        fi
    else
        echo "Notice: Optional Apple target '${target}' not present in toolchain target list; skipping."
    fi
done

echo ""
echo "Installing targets via rustup..."
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
echo "=== Step 3: Build Release Apple Static Libraries ==="
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-17.0}"
echo "IPHONEOS_DEPLOYMENT_TARGET=${IPHONEOS_DEPLOYMENT_TARGET}"

for target in "${TARGETS_TO_BUILD[@]}"; do
    echo "----------------------------------------"
    echo "Building release target: ${target}"
    cargo build -p synveil-ios-ffi --locked --release --target "${target}"

    LIB_PATH="target/${target}/release/libsynveil_ios_ffi.a"
    if [ -f "${LIB_PATH}" ]; then
        echo "SUCCESS: Produced release static library at ${LIB_PATH}"
        ls -lh "${LIB_PATH}"
    else
        echo "ERROR: Static library expected at ${LIB_PATH} was not found after build!" >&2
        exit 1
    fi
done

echo ""
echo "=== Step 4: Staging Root Initialization ==="
STAGING_DIR="target/ios-rust-artifacts"
echo "Recreating staging directory at ${STAGING_DIR}..."
rm -rf "${STAGING_DIR}"
mkdir -p "${STAGING_DIR}/device/arm64"
mkdir -p "${STAGING_DIR}/simulator/arm64"

if [ "${X86_64_SIM_AVAILABLE}" -eq 1 ]; then
    mkdir -p "${STAGING_DIR}/simulator/x86_64"
    mkdir -p "${STAGING_DIR}/simulator/universal"
fi

echo "Copying release libraries into staged layout..."
cp "target/aarch64-apple-ios/release/libsynveil_ios_ffi.a" "${STAGING_DIR}/device/arm64/libsynveil_ios_ffi.a"
cp "target/aarch64-apple-ios-sim/release/libsynveil_ios_ffi.a" "${STAGING_DIR}/simulator/arm64/libsynveil_ios_ffi.a"

if [ "${X86_64_SIM_AVAILABLE}" -eq 1 ]; then
    cp "target/x86_64-apple-ios/release/libsynveil_ios_ffi.a" "${STAGING_DIR}/simulator/x86_64/libsynveil_ios_ffi.a"
fi

echo ""
echo "=== Step 5: Architecture Verification & Universal Simulator Library ==="
if command -v lipo >/dev/null 2>&1; then
    echo "Verifying device/arm64..."
    DEV_INFO="$(lipo -info "${STAGING_DIR}/device/arm64/libsynveil_ios_ffi.a")"
    echo "  ${DEV_INFO}"
    if ! echo "${DEV_INFO}" | grep -q "arm64"; then
        echo "ERROR: Device library does not contain arm64 architecture!" >&2
        exit 1
    fi

    echo "Verifying simulator/arm64..."
    SIM_ARM_INFO="$(lipo -info "${STAGING_DIR}/simulator/arm64/libsynveil_ios_ffi.a")"
    echo "  ${SIM_ARM_INFO}"
    if ! echo "${SIM_ARM_INFO}" | grep -q "arm64"; then
        echo "ERROR: Simulator arm64 library does not contain arm64 architecture!" >&2
        exit 1
    fi

    if [ "${X86_64_SIM_AVAILABLE}" -eq 1 ]; then
        echo "Verifying simulator/x86_64..."
        SIM_X86_INFO="$(lipo -info "${STAGING_DIR}/simulator/x86_64/libsynveil_ios_ffi.a")"
        echo "  ${SIM_X86_INFO}"
        if ! echo "${SIM_X86_INFO}" | grep -q "x86_64"; then
            echo "ERROR: Simulator x86_64 library does not contain x86_64 architecture!" >&2
            exit 1
        fi

        echo "Creating simulator universal static library with lipo..."
        lipo -create \
            "${STAGING_DIR}/simulator/arm64/libsynveil_ios_ffi.a" \
            "${STAGING_DIR}/simulator/x86_64/libsynveil_ios_ffi.a" \
            -output "${STAGING_DIR}/simulator/universal/libsynveil_ios_ffi.a"

        echo "Verifying simulator/universal..."
        UNI_INFO="$(lipo -info "${STAGING_DIR}/simulator/universal/libsynveil_ios_ffi.a")"
        echo "  ${UNI_INFO}"
        if ! (echo "${UNI_INFO}" | grep -q "arm64" && echo "${UNI_INFO}" | grep -q "x86_64"); then
            echo "ERROR: Universal simulator library missing required architectures (arm64 + x86_64)!" >&2
            exit 1
        fi
    fi
else
    echo "Notice: lipo tool not present on host; skipping binary lipo verification/universal library creation."
fi

echo ""
echo "=== Step 6: Symbol Export Inspection ==="
if command -v nm >/dev/null 2>&1; then
    echo "Inspecting symbols for unexpected synveil_ffi_* C ABI exports..."
    SYMBOLS="$(nm -g "${STAGING_DIR}/device/arm64/libsynveil_ios_ffi.a" 2>/dev/null || true)"
    if echo "${SYMBOLS}" | grep -q "synveil_ffi_"; then
        echo "ERROR: Unexpected synveil_ffi_* C ABI symbol found prior to P016!" >&2
        echo "${SYMBOLS}" | grep "synveil_ffi_" >&2
        exit 1
    fi
    echo "Symbol inspection passed! No synveil_ffi_* C ABI symbols found."
else
    echo "Notice: nm tool not present on host; skipping nm symbol inspection."
fi

echo ""
echo "=== Step 7: Generate Manifest & Checksums ==="
PKG_VERSION="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import sys, json; data = json.load(sys.stdin); pkg = next((p for p in data.get("packages", []) if p.get("name") == "synveil-ios-ffi"), None); print(pkg["version"] if pkg else "0.1.0")')"
RUSTC_VER="$(rustc --version)"
CARGO_VER="$(cargo --version)"
COMMIT_SHA="${GITHUB_SHA:-$(git rev-parse HEAD 2>/dev/null || echo 'unknown')}"

python3 - <<EOF
import os, json, hashlib

staging_dir = "${STAGING_DIR}"
pkg_version = "${PKG_VERSION}"
rustc_ver = "${RUSTC_VER}"
cargo_ver = "${CARGO_VER}"
commit_sha = "${COMMIT_SHA}"
deployment_target = "${IPHONEOS_DEPLOYMENT_TARGET}"
x86_64_available = (${X86_64_SIM_AVAILABLE} == 1)

def file_info(rel_path, rust_target, platform_role, arch):
    full_path = os.path.join(staging_dir, rel_path)
    if not os.path.exists(full_path):
        return None
    size = os.path.getsize(full_path)
    h = hashlib.sha256()
    with open(full_path, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return {
        "relative_path": rel_path,
        "rust_target_triple": rust_target,
        "apple_platform_role": platform_role,
        "architecture": arch,
        "size_bytes": size,
        "sha256": h.hexdigest()
    }

variants = [
    file_info("device/arm64/libsynveil_ios_ffi.a", "aarch64-apple-ios", "device", "arm64"),
    file_info("simulator/arm64/libsynveil_ios_ffi.a", "aarch64-apple-ios-sim", "simulator", "arm64"),
]

if x86_64_available:
    variants.append(file_info("simulator/x86_64/libsynveil_ios_ffi.a", "x86_64-apple-ios", "simulator", "x86_64"))
    variants.append(file_info("simulator/universal/libsynveil_ios_ffi.a", "universal-simulator", "simulator", "arm64_x86_64"))

variants = [v for v in variants if v is not None]

manifest = {
    "schema_version": 1,
    "package_name": "synveil-ios-ffi",
    "package_version": pkg_version,
    "artifact_profile": "release",
    "minimum_ios_deployment_target": deployment_target,
    "rust_toolchain_version": rustc_ver,
    "cargo_version": cargo_ver,
    "source_commit_sha": commit_sha,
    "c_abi_export_status": "NONE_IN_P015",
    "cbindgen_status": "DEFERRED_TO_P016",
    "header_status": "NONE_IN_P015",
    "xcframework_status": "NONE_IN_P015",
    "variants": variants
}

manifest_path = os.path.join(staging_dir, "manifest.json")
with open(manifest_path, "w", encoding="utf-8") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")

# Generate SHA256SUMS
checksum_entries = []
for root, _, files in sorted(os.walk(staging_dir)):
    for file in sorted(files):
        if file == "SHA256SUMS":
            continue
        full_path = os.path.join(root, file)
        rel_path = os.path.relpath(full_path, staging_dir)
        h = hashlib.sha256()
        with open(full_path, "rb") as f:
            while chunk := f.read(65536):
                h.update(chunk)
        checksum_entries.append(f"{h.hexdigest()}  {rel_path}\n")

sums_path = os.path.join(staging_dir, "SHA256SUMS")
with open(sums_path, "w", encoding="utf-8") as f:
    f.writelines(sorted(checksum_entries))

print("Manifest and SHA256SUMS generated successfully.")
EOF

echo ""
echo "=== Step 8: Verify Checksums ==="
cd "${STAGING_DIR}"
if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c SHA256SUMS
elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c SHA256SUMS
else
    python3 -c "
import hashlib
with open('SHA256SUMS', 'r') as f:
    for line in f:
        expected, path = line.strip().split('  ', 1)
        h = hashlib.sha256()
        with open(path, 'rb') as pf:
            while chunk := pf.read(65536):
                h.update(chunk)
        if h.hexdigest() != expected:
            raise RuntimeError(f'Checksum mismatch for {path}')
print('Checksums verified via Python!')
"
fi
cd "${REPO_ROOT}"

echo ""
echo "=== Step 9: Verify Untracked Binary Output ==="
GIT_UNTRACKED_A="$(git status --short | grep '\.a$' || true)"
if [ -n "${GIT_UNTRACKED_A}" ]; then
    echo "ERROR: Compiled static library archives (.a) detected in git status:" >&2
    echo "${GIT_UNTRACKED_A}" >&2
    exit 1
fi

echo ""
echo "Synveil iOS Rust Apple Artifact Pipeline Completed Successfully!"
