#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(CDPATH='' cd -- "${SCRIPT_DIR}/../.." && pwd)"

exec python3 "${REPOSITORY_ROOT}/scripts/linux_quick_install.py" "$@"
