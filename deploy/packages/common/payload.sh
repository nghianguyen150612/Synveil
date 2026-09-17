#!/usr/bin/env bash
# payload.sh — package-neutral payload staging helper for native packaging (Prompt 79).
# Native DEB/RPM definitions MUST derive their payload from deploy/install/MANIFEST
# via deploy/install/install.sh, never from an independent artifact list.
# Sourced by build.sh; do NOT execute directly.
# SPDX-License-Identifier: MIT
set -euo pipefail

PACKAGES_PAYLOAD_COMMON_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGES_PAYLOAD_REPO_ROOT="$(cd "${PACKAGES_PAYLOAD_COMMON_DIR}/../../.." && pwd)"

# Stage the authoritative package payload into $1 (absolute staged root) using
# $2 as the maintenance binary, $3 as synveil-client, and $4 as synveil-desktop.
# Delegates to deploy/install/install.sh so MANIFEST remains the single source of truth.
synveil_stage_payload() {
    local staged_root="$1"
    local binary_src="$2"
    local client_binary_src="$3"
    local desktop_binary_src="$4"
    if [[ -z "$staged_root" || -z "$binary_src" || -z "$client_binary_src" || -z "$desktop_binary_src" ]]; then
        printf '[synveil-packages] ERROR: synveil_stage_payload requires <staged-root> <maintenance> <client> <desktop>\n' >&2
        return 1
    fi
    for source in "$binary_src" "$client_binary_src" "$desktop_binary_src"; do
        if [[ ! -x "$source" && ! -f "$source" ]]; then
            printf '[synveil-packages] ERROR: binary source missing: %s\n' "$source" >&2
            return 1
        fi
    done
    "${PACKAGES_PAYLOAD_REPO_ROOT}/deploy/install/install.sh" \
        "--root=${staged_root}" \
        "--binary=${binary_src}" \
        "--client-binary=${client_binary_src}" \
        "--desktop-binary=${desktop_binary_src}"
}

# Print the normalized expected payload manifest: "<dest> <mode> <class>" for every
# MANIFEST entry that produces a real file under the staged root (class PACKAGE).
# Used by build.sh and Rust parity tests to prove DEB == RPM == MANIFEST.
synveil_expected_package_files() {
    local manifest="${PACKAGES_PAYLOAD_REPO_ROOT}/deploy/install/MANIFEST"
    local line source dest mode _owner _group class
    while IFS= read -r line || [[ -n "$line" ]]; do
        line="${line#"${line%%[![:space:]]*}"}"
        line="${line%"${line##*[![:space:]]}"}"
        [[ -z "$line" ]] && continue
        [[ "$line" == \#* ]] && continue
        read -r source dest mode _owner _group class <<< "$line"
        if [[ "$class" == "PACKAGE" ]]; then
            printf '%s %s %s\n' "$dest" "$mode" "$class"
        fi
    done < "$manifest"
}

# Fail if any staged payload file under well-known secret paths exists with content.
# The package must create /etc/synveil and /etc/synveil/credentials directories only,
# never /etc/synveil/credentials/database-url with actual contents.
synveil_assert_no_secret_payload() {
    local staged_root="$1"
    local secret="${staged_root%/}/etc/synveil/credentials/database-url"
    if [[ -e "$secret" || -L "$secret" ]]; then
        printf '[synveil-packages] ERROR: secret payload present (must never be packaged): %s\n' "$secret" >&2
        return 1
    fi
    # Also reject any accidental DATABASE_URL-bearing env file seeded by packaging.
    # Administrator creates /etc/synveil/synveil-scheduled-maintenance.env; fresh
    # payload must leave it absent.
    local env_file="${staged_root%/}/etc/synveil/synveil-scheduled-maintenance.env"
    if [[ -e "$env_file" || -L "$env_file" ]]; then
        printf '[synveil-packages] ERROR: unexpected admin env file in payload (template lives at /usr/share): %s\n' "$env_file" >&2
        return 1
    fi
    return 0
}
