#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
python3 -m unittest discover -s "${ROOT}/tests/release_manifest" -p 'test_*.py' -v
