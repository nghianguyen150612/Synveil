#!/bin/bash -p
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
unset BASH_ENV ENV CDPATH PYTHONPATH PYTHONHOME

SCRIPT_DIR="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR

REPOSITORY_ROOT="$(CDPATH='' cd -- "${SCRIPT_DIR}/../.." && pwd)"
readonly REPOSITORY_ROOT

exec /usr/bin/python3 -E -s "${REPOSITORY_ROOT}/scripts/linux_quick_install.py" "$@"
