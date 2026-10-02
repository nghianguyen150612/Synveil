#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/linux_platform_detection.py --validate-policy
python3 -m unittest discover -s tests/linux_platform_detection -p 'test_*.py' -v
python3 -m py_compile scripts/linux_platform_detection.py
