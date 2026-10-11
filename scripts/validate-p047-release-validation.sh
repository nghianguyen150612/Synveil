#!/usr/bin/env bash
set -euo pipefail
python3 scripts/validate-p047-release-validation.py
python3 scripts/test-p047-release-validation.py
