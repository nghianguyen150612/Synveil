#!/usr/bin/env bash
# uninstall.sh — package-neutral uninstall / purge for Synveil desktop,
# background client, and scheduled-maintenance payloads.
# Prompt 76: ordinary uninstall removes PACKAGE artifacts only; purge is explicit.
# SPDX-License-Identifier: MIT
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=deploy/install/common.sh
source "${SCRIPT_DIR}/common.sh"

usage() {
    cat >&2 <<'EOF'
Usage: uninstall.sh --root=<staged-root> [--purge] [--help]

Removes Synveil PACKAGE-owned artifacts under the staged root.

Default (no --purge):
  Removes only PACKAGE files:
    /usr/bin/synveil-client
    /usr/bin/synveil-desktop
    /usr/bin/synveil-scheduled-maintenance-once
    /usr/lib/systemd/user/synveil-client.service
    /usr/lib/systemd/system/synveil-scheduled-maintenance.service
    /usr/lib/systemd/system/synveil-scheduled-maintenance.timer
    /usr/lib/sysusers.d/synveil.conf
    /usr/lib/tmpfiles.d/synveil.conf
    /usr/share/applications/synveil.desktop
    /usr/share/icons/hicolor/scalable/apps/synveil.svg
    /usr/share/doc/synveil/LICENSE and NOTICE
    /usr/share/synveil/synveil-scheduled-maintenance.env.example
  Preserves:
    /etc/synveil  (and any admin env file, non-secret)
    /etc/synveil/credentials (administrator secret source, root:root 0700)
    /etc/synveil/credentials/database-url credential file
    /var/lib/synveil (persistent state)
    external storage pools / user data / PostgreSQL
    shared parent directories (/usr/bin, /usr/lib/..., /etc, /var/lib)
    synveil system account (ordinary uninstall retains it)

With --purge (explicit destructive):
  After removing PACKAGE files, also removes:
    /etc/synveil  (administrator config + credentials)
    /var/lib/synveil (application runtime state)
  Still NEVER removes:
    external user-data/storage pools, backup destinations, object-store roots,
    mounted data volumes, external USB, home data, PostgreSQL databases.
  Symlinked purge targets are unlinked, not followed recursively.
  synveil account removal is DEFERRED (purge may log that manual userdel is
  safe only after state/config handling, but does not delete account automatically).

Options:
  --root=PATH     Staged root (DESTDIR) — absolute path. Required.
  --destdir=PATH  Alias for --root.
  --purge         Explicit destructive purge of config and state (not default).
  --help          Show help.

Environment:
  DESTDIR, SYNVEIL_ALLOW_HOST_ROOT as in install.sh.

Safety:
  - Validates staged root (absolute, no "..", not "/" without allow).
  - Validates each destination stays under root via realpath -m.
  - Unlinks known PACKAGE paths without following symlink targets; never
    recursively traverses symlink targets.
  - Never removes shared parent directories.
  - Path escape tests must pass.

Future real-host ordering (documented, not executed in staged test):
  systemctl disable --now synveil-scheduled-maintenance.timer
  systemctl stop synveil-scheduled-maintenance.service (allow bounded completion)
  <remove package artifacts via this script or package manager>
  systemctl daemon-reload
  # purge: explicit --purge after user confirms data loss
EOF
}

STAGED_ROOT="${DESTDIR:-}"
PURGE=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --root=*)
            STAGED_ROOT="${1#--root=}"
            shift
            ;;
        --destdir=*)
            STAGED_ROOT="${1#--destdir=}"
            shift
            ;;
        --purge)
            PURGE=1
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        --)
            shift
            break
            ;;
        -*)
            synveil_err "unknown option: $1"
            usage
            exit 2
            ;;
        *)
            synveil_err "unexpected argument: $1"
            usage
            exit 2
            ;;
    esac
done

if [[ -z "$STAGED_ROOT" ]]; then
    synveil_err "--root is required (e.g., --root=/tmp/synveil-root)"
    usage
    exit 2
fi

synveil_validate_root "$STAGED_ROOT" || exit 1
# For uninstall, root should exist; if not, nothing to do but still validate.
if [[ ! -e "$STAGED_ROOT" ]]; then
    synveil_log "staged root does not exist: $STAGED_ROOT (nothing to uninstall)"
    exit 0
fi
if [[ -L "$STAGED_ROOT" ]]; then
    synveil_err "staged root must not be a symlink: $STAGED_ROOT"
    exit 1
fi

# Convert to canonical for containment checks (realpath -m if available)
if command -v realpath >/dev/null 2>&1; then
    CANON_ROOT="$(realpath -m "$STAGED_ROOT")"
else
    CANON_ROOT="$STAGED_ROOT"
fi

synveil_log "uninstalling Synveil (purge=$PURGE) from staged root: $STAGED_ROOT"

# ---------------------------------------------------------------------------
# Helper: safe unlink of a known PACKAGE path (no following, no recursion)
# ---------------------------------------------------------------------------
safe_unlink_package() {
    local dest="$1"
    synveil_dest_under_root_lexical "$STAGED_ROOT" "$dest" || return 1
    synveil_dest_under_root "$STAGED_ROOT" "$dest" || return 1
    local full="${STAGED_ROOT%/}${dest}"
    if [[ -L "$full" ]]; then
        synveil_log "unlink symlink PACKAGE $dest -> $(readlink "$full" 2>/dev/null || echo "?") (not following target)"
        rm -f "$full"
        return 0
    fi
    if [[ -f "$full" ]]; then
        synveil_log "remove PACKAGE file $dest"
        rm -f "$full"
        return 0
    fi
    if [[ -e "$full" ]]; then
        # Unexpected type (directory, etc.) at file path — remove only if it's a regular file-like; do not recurse.
        synveil_log "PACKAGE path $dest exists but is not regular file/symlink (type $(stat -c %F "$full" 2>/dev/null || echo unknown)); unlinking without recursion"
        rm -f "$full" 2>/dev/null || {
            synveil_err "refusing to recursively delete non-file PACKAGE path $dest"
            return 1
        }
        return 0
    fi
    synveil_log "PACKAGE $dest already absent (idempotent)"
    return 0
}

# ---------------------------------------------------------------------------
# Phase 1: remove PACKAGE artifacts (manifest class PACKAGE)
# ---------------------------------------------------------------------------
while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [[ -z "$line" ]] && continue
    [[ "$line" == \#* ]] && continue
    read -r _source dest _mode _owner _group class <<< "$line"
    if [[ "$class" == "PACKAGE" ]]; then
        safe_unlink_package "$dest" || { synveil_err "failed to remove $dest"; exit 1; }
    fi
done < "$MANIFEST_FILE"

# ---------------------------------------------------------------------------
# Phase 2: handle CONFIG_DIRECTORY / STATE_DIRECTORY
# ---------------------------------------------------------------------------
handle_purge_target() {
    local dest="$1"
    local label="$2"
    synveil_dest_under_root_lexical "$STAGED_ROOT" "$dest" || return 1
    local full="${STAGED_ROOT%/}${dest}"
    if [[ ! -e "$full" && ! -L "$full" ]]; then
        synveil_log "$label $dest already absent"
        return 0
    fi
    # Symlink safety: if target is symlink, unlink only the link.
    if [[ -L "$full" ]]; then
        local target
        target="$(readlink "$full" 2>/dev/null || echo unknown)"
        synveil_log "purge: $label $dest is symlink -> $target; unlinking link only (not target)"
        rm -f "$full"
        return 0
    fi
    # Ensure canonical target is still under root before rm -rf
    local canon_target
    if command -v realpath >/dev/null 2>&1; then
        canon_target="$(realpath -m "$full")"
    else
        canon_target="$full"
    fi
    if [[ "$canon_target" != "$CANON_ROOT" && "$canon_target" != "$CANON_ROOT"/* ]]; then
        synveil_err "purge target escapes staged root: $dest -> $canon_target not under $CANON_ROOT; refusing"
        return 1
    fi
    # Additional safety: refuse to purge if dest is "/" or parent des like "/etc" "/var/lib" without synveil suffix
    if [[ "$dest" == "/" || "$dest" == "/etc" || "$dest" == "/var/lib" || "$dest" == "/var" || "$dest" == "/usr" ]]; then
        synveil_err "refusing to purge shared parent directory: $dest"
        return 1
    fi
    # Only allow known Synveil state/config paths
    if [[ "$dest" != "/etc/synveil" && "$dest" != "/var/lib/synveil" && "$dest" != "/etc/synveil/credentials" ]]; then
        synveil_err "purge target not in allowlist, refusing: $dest"
        return 1
    fi
    synveil_log "purge: recursively removing $label $dest (explicit --purge)"
    rm -rf "$full"
}

if [[ "$PURGE" -eq 1 ]]; then
    handle_purge_target "/etc/synveil" "CONFIG_DIRECTORY" || exit 1
    # /etc/synveil removal already includes credentials, but handle explicitly for idempotence
    handle_purge_target "/var/lib/synveil" "STATE_DIRECTORY" || exit 1
    synveil_log "purge: completed; external user-data/storage pools were NOT touched (invariant)"
    cat >&2 <<EOF
[synveil-install] purge complete for $STAGED_ROOT
  Note: synveil system account is RETAINED by default (even on purge). If no
  Synveil-owned files remain, administrator may manually run:
    sudo userdel synveil  # only after purge and confirming no orphaned UID files
  This script does NOT delete the account automatically to avoid orphaned ownership.
  Future distro packaging may handle account lifecycle explicitly.
EOF
else
    # Ordinary uninstall preserves config/state/credentials
    for keep in "/etc/synveil" "/etc/synveil/credentials" "/var/lib/synveil"; do
        local_full="${STAGED_ROOT%/}${keep}"
        if [[ -e "$local_full" || -L "$local_full" ]]; then
            synveil_log "preserve $keep (ordinary uninstall keeps config/credentials/state; use --purge for explicit removal)"
        fi
    done
    synveil_log "ordinary uninstall complete; config/credentials/state preserved; account retained"
fi

# ---------------------------------------------------------------------------
# Phase 3: parent directory safety check (never remove shared parents)
# ---------------------------------------------------------------------------
for parent in "/usr/bin" "/usr/lib/systemd/user" "/usr/lib/systemd/system" "/usr/lib/sysusers.d" "/usr/lib/tmpfiles.d" "/usr/share/applications" "/usr/share/icons/hicolor/scalable/apps" "/usr/share/doc/synveil" "/usr/share/synveil" "/etc" "/var/lib"; do
    parent_full="${STAGED_ROOT%/}${parent}"
    if [[ ! -e "$parent_full" && ! -L "$parent_full" ]]; then
        # If parent was never created because we used install -D, it's okay it doesn't exist after uninstall,
        # but we must not have removed it if it existed before install.
        # We only guarantee we didn't rm -rf the parent itself. This check logs.
        synveil_log "parent $parent not present under staged root (not removed by us if it was pre-existing shared dir)"
    else
        synveil_log "parent $parent preserved (not recursively removed)"
    fi
done

# ---------------------------------------------------------------------------
# Final notes
# ---------------------------------------------------------------------------
cat >&2 <<EOF
[synveil-install] uninstall complete (purge=$PURGE) for $STAGED_ROOT
  Removed: PACKAGE artifacts (desktop/client/maintenance binaries, units, integration metadata, notices)
  Preserved by default: /etc/synveil, /var/lib/synveil, external pools, PostgreSQL, parent dirs, account
  Purge requires explicit --purge flag (still preserves external pools)
EOF

exit 0
