#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/validate-install-error-model.py
python3 tests/install-error-model/test_acquisition_bridge.py
cargo test -p synveil-install-engine --locked --test error_model_contract
