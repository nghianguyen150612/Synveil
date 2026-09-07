#!/usr/bin/env bash
# arch.sh — deterministic host-architecture mapping for native packaging (Prompt 79).
# No cross-compilation in Prompt 79: only the current host architecture is claimed.
# Sourced by build.sh; do NOT execute directly.
# SPDX-License-Identifier: MIT
set -euo pipefail

# Print Debian architecture for a given uname -m value (default: current host).
# Supported: x86_64 -> amd64, aarch64 -> arm64 (purely declarative).
synveil_deb_arch() {
    local machine="${1:-$(uname -m)}"
    case "$machine" in
        x86_64|amd64)
            printf 'amd64'
            ;;
        aarch64|arm64)
            printf 'arm64'
            ;;
        *)
            printf '[synveil-packages] ERROR: unsupported architecture for DEB: %s (Prompt 79 supports x86_64, declaratively aarch64)\n' "$machine" >&2
            return 1
            ;;
    esac
}

# Print RPM architecture for a given uname -m value (default: current host).
# Supported: x86_64 -> x86_64, aarch64 -> aarch64 (purely declarative).
synveil_rpm_arch() {
    local machine="${1:-$(uname -m)}"
    case "$machine" in
        x86_64)
            printf 'x86_64'
            ;;
        aarch64|arm64)
            printf 'aarch64'
            ;;
        *)
            printf '[synveil-packages] ERROR: unsupported architecture for RPM: %s (Prompt 79 supports x86_64, declaratively aarch64)\n' "$machine" >&2
            return 1
            ;;
    esac
}
