#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "${REPO_ROOT}"

echo "=== Preparing iOS Rust Static Libraries for Xcode ==="
"${REPO_ROOT}/scripts/generate-ios-rust-header.sh"
"${REPO_ROOT}/scripts/build-ios-rust-artifacts.sh"
python3 "${REPO_ROOT}/scripts/validate_ios_rust_artifact.py"
