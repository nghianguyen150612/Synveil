#!/usr/bin/env bash
# version.sh — canonical version source for native Linux packaging (Prompt 79).
# Derives DEB/RPM version deterministically from workspace Cargo.toml.
# Sourced by build.sh and packaging tests; do NOT execute directly.
# SPDX-License-Identifier: MIT
set -euo pipefail

# Resolve repository root from this script's location.
# version.sh lives at deploy/packages/common/version.sh → repo root is ../../..
PACKAGES_COMMON_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGES_REPO_ROOT="$(cd "${PACKAGES_COMMON_DIR}/../../.." && pwd)"

# Print raw workspace.package.version from Cargo.toml (e.g. "0.1.0").
# Fails if the version cannot be determined — never falls back to a hardcoded value.
synveil_cargo_version() {
    local cargo_toml="${PACKAGES_REPO_ROOT}/Cargo.toml"
    if [[ ! -f "$cargo_toml" ]]; then
        printf '[synveil-packages] ERROR: Cargo.toml not found: %s\n' "$cargo_toml" >&2
        return 1
    fi
    # Parse [workspace.package] version = "X" without depending on cargo/toml tools.
    # Only the line inside the [workspace.package] section counts.
    local version=""
    local in_section=0
    local line
    while IFS= read -r line || [[ -n "$line" ]]; do
        # Trim leading whitespace for section detection.
        local trimmed="${line#"${line%%[![:space:]]*}"}"
        if [[ "$trimmed" == \[* ]]; then
            if [[ "$trimmed" == "[workspace.package]"* ]]; then
                in_section=1
            else
                in_section=0
            fi
            continue
        fi
        if [[ "$in_section" -eq 1 ]]; then
            # Match: version = "0.1.0" (allow spaces, single or double quotes)
            if [[ "$trimmed" == version* ]]; then
                # Extract quoted value.
                local value
                value="$(printf '%s' "$trimmed" | sed -n 's/^version[[:space:]]*=[[:space:]]*["'\'']\([^"'\'']*\)["'\''].*/\1/p')"
                if [[ -n "$value" ]]; then
                    version="$value"
                    break
                fi
            fi
        fi
    done < "$cargo_toml"
    if [[ -z "$version" ]]; then
        printf '[synveil-packages] ERROR: workspace.package.version not found in %s\n' "$cargo_toml" >&2
        return 1
    fi
    printf '%s' "$version"
}

# Normalize a Cargo SemVer into valid DEB upstream_version syntax.
# Rules (deterministic, documented):
#   - DEB upstream version may contain alphanumerics, '.', '+', '-', '~', ':' is epoch separator (not used here).
#   - Cargo pre-release uses '-' (e.g. 1.0.0-beta.1); DEB orders '~' before everything, so map '-' to '~'
#     ONLY in the pre-release portion? Simplest deterministic rule that preserves ordering intent:
#     replace '-' with '~' and '+' stays '+' (both valid in DEB). Build metadata after '+' is kept.
#   - For plain "0.1.0" the output equals the input.
synveil_deb_version() {
    local cargo_version="$1"
    if [[ -z "$cargo_version" ]]; then
        printf '[synveil-packages] ERROR: cargo version must not be empty\n' >&2
        return 1
    fi
    # Validate SemVer-ish shape: digits.digits.digits with optional -/+ suffix.
    if [[ ! "$cargo_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][A-Za-z0-9.+~-]+)?$ ]]; then
        printf '[synveil-packages] ERROR: cargo version %q is not SemVer-shaped\n' "$cargo_version" >&2
        return 1
    fi
    # Map '-' to '~' for DEB ordering (deterministic). '+' is valid in both, keep.
    local deb_version="${cargo_version//-/\~}"
    printf '%s' "$deb_version"
}

# Normalize a Cargo SemVer into valid RPM Version: tag syntax.
# Rules (deterministic):
#   - RPM Version must not contain '-' (dash separates version-release). Map '-' to '~'
#     (RPM supports '~' for pre-release ordering since rpm 4.10).
#   - '+' is allowed in RPM Version; keep.
#   - For plain "0.1.0" the output equals the input.
synveil_rpm_version() {
    local cargo_version="$1"
    if [[ -z "$cargo_version" ]]; then
        printf '[synveil-packages] ERROR: cargo version must not be empty\n' >&2
        return 1
    fi
    if [[ ! "$cargo_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][A-Za-z0-9.+~_]+)?$ ]]; then
        printf '[synveil-packages] ERROR: cargo version %q is not SemVer-shaped\n' "$cargo_version" >&2
        return 1
    fi
    local rpm_version="${cargo_version//-/\~}"
    printf '%s' "$rpm_version"
}

# RPM Release tag for Gen-1 (deterministic, no hardcoded version inside).
# Always "1" unless overridden by test-only packaging fixture via env (never by editing Cargo version).
synveil_rpm_release() {
    printf '%s' "${SYNVEIL_RPM_RELEASE:-1}"
}
