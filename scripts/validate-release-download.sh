#!/usr/bin/env bash
set -euo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
python3 -m json.tool deploy/release/release-auth-v1.schema.json >/dev/null
python3 -m py_compile scripts/release_download.py tests/release_download/test_release_download.py
python3 -m unittest discover -s tests/release_download -p 'test_*.py' -v
./scripts/validate-release-manifest.sh
