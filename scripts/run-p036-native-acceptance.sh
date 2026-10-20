#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "$0")" && pwd -P)"
ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"
PLATFORM="${1:-}"
if [[ "$PLATFORM" != ubuntu-24.04 && "$PLATFORM" != fedora-42 ]]; then
  printf '%s\n' 'BLOCKED_ENVIRONMENT: expected ubuntu-24.04 or fedora-42' >&2
  exit 2
fi
if [[ ! -f "$ROOT/acceptance/p036/production-artifacts.json" ]]; then
  printf '%s\n' 'BLOCKED_BY_EXTERNAL_INPUT: acceptance/p036/production-artifacts.json has not been supplied' >&2
  exit 2
fi
exec python3 "$ROOT/scripts/p036_native_acceptance.py" "$PLATFORM"
