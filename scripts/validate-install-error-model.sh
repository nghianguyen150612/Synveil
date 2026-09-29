#!/usr/bin/env bash
set -euo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
python3 scripts/validate-install-error-model.py
python3 tests/install-error-model/test_acquisition_bridge.py
cargo test -p synveil-install-engine --test error_model_contract --locked
