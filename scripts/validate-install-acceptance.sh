#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
python3 scripts/install_acceptance.py validate
python3 scripts/install_acceptance.py self-test
