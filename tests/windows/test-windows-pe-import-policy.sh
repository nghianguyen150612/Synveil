#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
# shellcheck source=deploy/packages/common/windows-pe-import-policy.sh
source "${ROOT}/deploy/packages/common/windows-pe-import-policy.sh"

for system_dll in UIAutomationCore.dll kernel32.dll api-ms-win-core-file-l1-1-0.dll \
    ncrypt.dll NCRYPT.DLL Cabinet.dll CABINET.DLL Msi.dll MSI.DLL Wininet.dll WININET.DLL \
    Wintrust.dll WINTRUST.DLL; do
    if ! is_system_dll "$system_dll"; then
        printf 'expected Windows system import to be allowed: %s\n' "$system_dll" >&2
        exit 1
    fi
done

for bundled_dll in MSVCP140.dll vcruntime140.dll synveil-unknown.dll arbitrary-unknown.dll \
    cabinet-unknown.dll msi-unknown.dll; do
    if is_system_dll "$bundled_dll"; then
        printf 'expected non-system import to require a packaged DLL: %s\n' "$bundled_dll" >&2
        exit 1
    fi
done

printf 'Windows PE import policy: PASS\n'
