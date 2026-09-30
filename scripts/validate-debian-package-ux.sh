#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

python3 scripts/validate-debian-package-ux.py "$@"
cargo test -p synveil-metadata --test debian_package_ux_units --locked -- --nocapture
