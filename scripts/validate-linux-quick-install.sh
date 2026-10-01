#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
python3 -m unittest discover -s tests/linux_quick_install -p 'test_*.py' -v
python3 -m unittest discover -s tests/release_download -p 'test_*.py' -v
python3 -m unittest discover -s tests/release_channel -p 'test_*.py' -v
bash -n deploy/install/quick-install.sh
python3 -m py_compile scripts/linux_quick_install.py
