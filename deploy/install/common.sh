#!/usr/bin/env bash
# common.sh — shared helpers for Synveil package-neutral install lifecycle (Prompt 76)
# Sourced by install.sh and uninstall.sh; do NOT execute directly.
# SPDX-License-Identifier: MIT
set -euo pipefail

# Resolve repository root from this script's location.
# common.sh lives at deploy/install/common.sh → repo root is ../..
COMMON_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Consumed by install.sh/uninstall.sh after this file is sourced.
# shellcheck disable=SC2034
REPO_ROOT="$(cd "${COMMON_DIR}/../.." && pwd)"
MANIFEST_FILE="${COMMON_DIR}/MANIFEST"

# ---------------------------------------------------------------------------
# Logging (stderr, no secrets)
# ---------------------------------------------------------------------------
synveil_log() {
    printf '[synveil-install] %s\n' "$*" >&2
}
synveil_err() {
    printf '[synveil-install] ERROR: %s\n' "$*" >&2
}

# ---------------------------------------------------------------------------
# Path safety helpers
# ---------------------------------------------------------------------------

# Validate that STAGED_ROOT is usable and safe.
# Refuses empty, relative, "/" without explicit allow, ".." components, and
# non-absolute values. This prevents writes outside the intended root.
synveil_validate_root() {
    local root="$1"
    if [[ -z "${root:-}" ]]; then
        synveil_err "staged root must not be empty (pass --root=/tmp/root or set DESTDIR)"
        return 1
    fi
    if [[ "$root" != /* ]]; then
        synveil_err "staged root must be absolute, got: $root"
        return 1
    fi
    # Reject bare "/" unless caller explicitly allows host mutation.
    if [[ "$root" == "/" && "${SYNVEIL_ALLOW_HOST_ROOT:-0}" != "1" ]]; then
        synveil_err "refusing to operate on host root / without SYNVEIL_ALLOW_HOST_ROOT=1 (use staged root like /tmp/synveil-root)"
        return 1
    fi
    # Reject any ".." path component — prevents escape like /tmp/root/../etc
    if [[ "$root" == *".."* ]]; then
        # More precise: split components and check for exactly ".."
        local IFS='/'
        local part
        # shellcheck disable=SC2162
        read -ra parts <<< "$root"
        for part in "${parts[@]}"; do
            if [[ "$part" == ".." ]]; then
                synveil_err "staged root must not contain '..' component, got: $root"
                return 1
            fi
        done
    fi
    # Reject empty components that would be "//"? realpath -m will normalize,
    # but we keep simple: disallow trailing /. except for root "/"
    return 0
}

# Ensure destination stays under root (defends against absolute/relative mishandling,
# ".." in destination, and symlink escape via realpath -m).
# Usage: synveil_dest_under_root <root> <destination>
# For install paths, we want to ensure the intended destination path itself (lexically)
# is under root. For uninstall unlink paths where the file may already be a symlink,
# we must NOT follow the symlink target when checking — the link location itself must
# be under root, not its target.
synveil_dest_under_root() {
    local root="$1"
    local dest="$2"
    if [[ "$dest" != /* ]]; then
        synveil_err "destination must be absolute, got: $dest"
        return 1
    fi
    if [[ "$dest" == *".."* ]]; then
        local IFS='/'
        local part
        read -ra parts <<< "$dest"
        for part in "${parts[@]}"; do
            if [[ "$part" == ".." ]]; then
                synveil_err "destination must not contain '..', got: $dest"
                return 1
            fi
        done
    fi
    local combined="${root%/}${dest}"
    # Lexical check first: combined must start with root prefix (without symlink expansion)
    # Normalize ".." and "." lexically via realpath -m -s (strip symlinks) if available.
    local canon_combined_lex
    local canon_root_lex
    if command -v realpath >/dev/null 2>&1; then
        # -s: don't expand symlinks (lexical), -m: allow missing components
        if realpath -m -s "$combined" >/dev/null 2>&1; then
            canon_combined_lex="$(realpath -m -s "$combined")"
            canon_root_lex="$(realpath -m -s "$root")"
        else
            # Older realpath without -s: fallback to -m (may follow final symlink, but lexical part still covered)
            canon_combined_lex="$(realpath -m "$combined")"
            canon_root_lex="$(realpath -m "$root")"
        fi
    else
        canon_combined_lex="$combined"
        canon_root_lex="$root"
    fi
    if [[ "$canon_combined_lex" != "$canon_root_lex" && "$canon_combined_lex" != "$canon_root_lex"/* ]]; then
        synveil_err "destination escapes staged root (lexical): root=$root dest=$dest -> $canon_combined_lex not under $canon_root_lex"
        return 1
    fi
    # For install safety, also check that parent directory's real (symlink-resolved) location is under root
    # to catch parent-dir symlink escape (e.g., $root/usr -> /etc). We check dirname separately.
    local parent_dir
    parent_dir="$(dirname "$combined")"
    if [[ -e "$parent_dir" ]]; then
        local canon_parent_real
        local canon_root_real
        if command -v realpath >/dev/null 2>&1; then
            # realpath without -s follows symlinks to detect escape
            canon_parent_real="$(realpath -m "$parent_dir" 2>/dev/null || echo "$parent_dir")"
            canon_root_real="$(realpath -m "$root" 2>/dev/null || echo "$root")"
            if [[ "$canon_parent_real" != "$canon_root_real" && "$canon_parent_real" != "$canon_root_real"/* ]]; then
                # If parent is a symlink outside root, reject install/unlink that would write outside
                # For uninstall of a file that is itself a symlink, the parent check will still fail if parent is symlinked outside.
                # That's correct: we should not operate on a parent that escapes.
                # However, for file unlink where file is symlink, parent may still be inside root; this check is fine.
                synveil_err "destination parent escapes staged root: parent $parent_dir -> $canon_parent_real not under $canon_root_real"
                return 1
            fi
        fi
    fi
    return 0
}

# Lexical-only check for uninstall unlink (does not follow final symlink target).
# Allows unlinking a symlink file whose target points outside, as long as the link
# location itself is under root.
synveil_dest_under_root_lexical() {
    local root="$1"
    local dest="$2"
    if [[ "$dest" != /* ]]; then
        synveil_err "destination must be absolute, got: $dest"
        return 1
    fi
    if [[ "$dest" == *".."* ]]; then
        local IFS='/'
        local part
        read -ra parts <<< "$dest"
        for part in "${parts[@]}"; do
            if [[ "$part" == ".." ]]; then
                synveil_err "destination must not contain '..', got: $dest"
                return 1
            fi
        done
    fi
    local combined="${root%/}${dest}"
    local canon_combined_lex
    local canon_root_lex
    if command -v realpath >/dev/null 2>&1; then
        if realpath -m -s "$combined" >/dev/null 2>&1; then
            canon_combined_lex="$(realpath -m -s "$combined")"
            canon_root_lex="$(realpath -m -s "$root")"
        else
            canon_combined_lex="$(realpath -m "$combined")"
            canon_root_lex="$(realpath -m "$root")"
        fi
    else
        canon_combined_lex="$combined"
        canon_root_lex="$root"
    fi
    if [[ "$canon_combined_lex" != "$canon_root_lex" && "$canon_combined_lex" != "$canon_root_lex"/* ]]; then
        synveil_err "destination escapes staged root (lexical): root=$root dest=$dest -> $canon_combined_lex not under $canon_root_lex"
        return 1
    fi
    return 0
}

# Validate staged root before destructive operations: ensure it exists (or can be
# created) and is not a symlink to outside.
synveil_ensure_root_exists() {
    local root="$1"
    if [[ -L "$root" ]]; then
        synveil_err "staged root must not be a symlink, got: $root"
        return 1
    fi
    # mkdir -p the root so subsequent operations have a place; mode 0755 is fine for staging.
    mkdir -p "$root"
}

# ---------------------------------------------------------------------------
# Atomic install helper
# ---------------------------------------------------------------------------
# Install file atomically: write to temp then rename.
# Args: src dest mode owner group
# Owner/group chown is attempted only if running as root; in rootless staged tests
# we validate intended ownership via manifest/mode checks separately.
synveil_atomic_install() {
    local src="$1"
    local dest="$2"
    local mode="$3"
    local owner="$4"
    local group="$5"

    if [[ ! -e "$src" ]]; then
        synveil_err "source artifact missing: $src"
        return 1
    fi
    local dest_dir
    dest_dir="$(dirname "$dest")"
    mkdir -p "$dest_dir"
    local tmp="${dest}.tmp.$$"
    # Ensure tmp is cleaned on failure
    rm -f "$tmp"
    # Use cp + chmod for portability; install(1) semantics are similar but cp is more explicit for atomic rename.
    cp -a "$src" "$tmp"
    chmod "$mode" "$tmp"
    # Only attempt chown if we are root; otherwise record intended ownership via manifest and skip.
    if [[ "$(id -u)" -eq 0 ]]; then
        if ! chown "${owner}:${group}" "$tmp" 2>/dev/null; then
            synveil_err "chown ${owner}:${group} failed for $tmp (user/group may not exist in staging); continuing with mode only (staged test)"
        fi
    fi
    # Atomic rename — POSIX mv over same filesystem is atomic.
    mv -f "$tmp" "$dest"
    # Ensure final mode (mv preserves mode of tmp)
    chmod "$mode" "$dest" 2>/dev/null || true
    if [[ "$(id -u)" -eq 0 ]]; then
        chown "${owner}:${group}" "$dest" 2>/dev/null || true
    fi
}

# ---------------------------------------------------------------------------
# Manifest parsing
# ---------------------------------------------------------------------------
# Iterate manifest lines, skipping comments/empty, invoking callback for each entry.
# Callback receives: source dest mode owner group class
# Shell note: MANIFEST fields are whitespace-separated; destination is field 2, etc.
synveil_manifest_each() {
    local callback="$1"
    local line source dest mode owner group class
    while IFS= read -r line || [[ -n "$line" ]]; do
        # Trim leading/trailing whitespace
        line="${line#"${line%%[![:space:]]*}"}"
        line="${line%"${line##*[![:space:]]}"}"
        [[ -z "$line" ]] && continue
        [[ "$line" == \#* ]] && continue
        # Split into 6 fields (source dest mode owner group class)
        # Use read with default IFS (space/tab)
        # Guard against malformed lines.
        read -r source dest mode owner group class <<< "$line"
        if [[ -z "$source" || -z "$dest" || -z "$mode" || -z "$owner" || -z "$group" || -z "$class" ]]; then
            synveil_err "malformed manifest line: $line"
            return 1
        fi
        "$callback" "$source" "$dest" "$mode" "$owner" "$group" "$class"
    done < "$MANIFEST_FILE"
}

# Count package-owned artifacts for failure injection and validation
synveil_count_package_entries() {
    local count=0
    while IFS= read -r line || [[ -n "$line" ]]; do
        line="${line#"${line%%[![:space:]]*}"}"
        [[ -z "$line" ]] && continue
        [[ "$line" == \#* ]] && continue
        read -r source dest mode owner group class <<< "$line"
        if [[ "$class" == "PACKAGE" ]]; then
            count=$((count + 1))
        fi
    done < "$MANIFEST_FILE"
    printf '%s' "$count"
}
