#!/usr/bin/env bash
# install.sh — package-neutral Linux installation lifecycle for Synveil desktop
# and scheduled-maintenance payloads. Prompt 76 lifecycle guarantees remain
# unchanged; Prompt 99 adds the separate desktop/client/user-unit payload.
# Distribution packages (.deb/.rpm/etc.) MUST call this layer with DESTDIR/--root.
# SPDX-License-Identifier: MIT
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=deploy/install/common.sh
source "${SCRIPT_DIR}/common.sh"

usage() {
    cat >&2 <<'EOF'
Usage: install.sh --root=<staged-root> [--binary=<maintenance>] \
                  [--client-binary=<synveil-client>] [--desktop-binary=<synveil-desktop>]
                  [--destdir=<staged-root>] [--help]

Package-neutral Synveil installation. Installs PACKAGE-owned artifacts and
creates CONFIG_DIRECTORY skeleton under the staged root. Never starts systemd
units, never creates host users, never modifies host filesystem unless
--root=/ is explicitly allowed via SYNVEIL_ALLOW_HOST_ROOT=1.

Options:
  --root=PATH        Staged root (DESTDIR) — absolute path under /tmp etc. Required.
  --destdir=PATH     Alias for --root (for DESTDIR compatibility).
  --binary=PATH      Path to built synveil-scheduled-maintenance-once binary.
                     Defaults to $REPO_ROOT/target/debug/synveil-scheduled-maintenance-once
                     or $SYNVEIL_INSTALL_BINARY if set.
  --client-binary=PATH
                     Path to the packaged synveil-client executable. Defaults
                     to target/debug or target/release, or
                     $SYNVEIL_INSTALL_CLIENT_BINARY.
  --desktop-binary=PATH
                     Path to the packaged synveil-desktop executable. Defaults
                     to target/debug or target/release, or
                     $SYNVEIL_INSTALL_DESKTOP_BINARY.
  --help             Show this help.

Environment:
  DESTDIR            Alternative staged root (like make DESTDIR=).
  SYNVEIL_INSTALL_BINARY  Path to binary artifact if --binary not given.
  SYNVEIL_INSTALL_CLIENT_BINARY  Path to synveil-client if not specified.
  SYNVEIL_INSTALL_DESKTOP_BINARY  Path to synveil-desktop if not specified.
  SYNVEIL_INSTALL_FAIL_AFTER=N  Test-only: fail after N PACKAGE artifacts (simulates mid-upgrade failure).
  SYNVEIL_ALLOW_HOST_ROOT=1    Allow --root=/ (host root) for real packaging.

Lifecycle:
  - PACKAGE files are atomically replaced (write temp + rename), not truncated in place.
  - CONFIG_DIRECTORY /etc/synveil is created with 0750 root:synveil, never overwrites existing
    /etc/synveil/synveil-scheduled-maintenance.env (admin-owned).
  - CREDENTIAL_DIRECTORY /etc/synveil/credentials is created with 0700 root:root, never
    creates a secret file; admin provisions database-url as root:root 0600 and LoadCredential
    exposes per-service copy (synveil does not need direct read on source).
  - STATE_DIRECTORY /var/lib/synveil and RUNTIME_MANAGED /run/synveil are NOT created
    by this script; they are managed by tmpfiles.d / RuntimeDirectory= (Prompt 75). This
    preserves the ownership contract and avoids seeding fake state.
  - Idempotent: running twice yields identical PACKAGE-owned state.
  - Upgrade: re-invoking with newer binary/artifact content atomically replaces PACKAGE files
    while preserving /etc/synveil, /etc/synveil/credentials/database-url and /var/lib/synveil.
  - Failure: if SYNVEIL_INSTALL_FAIL_AFTER triggers, already-replaced PACKAGE files remain
    at new version (partial upgrade); config/state/credential/data remain untouched. Full transactional
    rollback is DEFERRED to distro package manager; this layer guarantees data preservation.

See deploy/install/MANIFEST, deploy/README.md, docs/en/DEPLOYMENT.md.
EOF
}

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
STAGED_ROOT="${DESTDIR:-}"
BINARY_SRC="${SYNVEIL_INSTALL_BINARY:-}"
CLIENT_BINARY_SRC="${SYNVEIL_INSTALL_CLIENT_BINARY:-}"
DESKTOP_BINARY_SRC="${SYNVEIL_INSTALL_DESKTOP_BINARY:-}"

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
        --binary=*)
            BINARY_SRC="${1#--binary=}"
            shift
            ;;
        --client-binary=*)
            CLIENT_BINARY_SRC="${1#--client-binary=}"
            shift
            ;;
        --desktop-binary=*)
            DESKTOP_BINARY_SRC="${1#--desktop-binary=}"
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
            synveil_err "unexpected argument: $1 (use --root=PATH)"
            usage
            exit 2
            ;;
    esac
done

# Handle DESTDIR env still preferred if not overridden
if [[ -z "$STAGED_ROOT" && -n "${DESTDIR:-}" ]]; then
    STAGED_ROOT="$DESTDIR"
fi

if [[ -z "$STAGED_ROOT" ]]; then
    synveil_err "--root is required (e.g., --root=/tmp/synveil-root). Refusing to guess host root."
    usage
    exit 2
fi

synveil_validate_root "$STAGED_ROOT" || exit 1
synveil_ensure_root_exists "$STAGED_ROOT" || exit 1

# Resolve binary source
if [[ -z "$BINARY_SRC" ]]; then
    # Default: look for built artifact at target/debug or target/release
    if [[ -x "${REPO_ROOT}/target/debug/synveil-scheduled-maintenance-once" ]]; then
        BINARY_SRC="${REPO_ROOT}/target/debug/synveil-scheduled-maintenance-once"
    elif [[ -x "${REPO_ROOT}/target/release/synveil-scheduled-maintenance-once" ]]; then
        BINARY_SRC="${REPO_ROOT}/target/release/synveil-scheduled-maintenance-once"
    else
        synveil_err "binary artifact not found; pass --binary=PATH (expected target/debug/synveil-scheduled-maintenance-once built from crates/api/src/bin/synveil-scheduled-maintenance-once.rs)"
        exit 1
    fi
fi

if [[ ! -f "$BINARY_SRC" ]]; then
    synveil_err "binary source does not exist: $BINARY_SRC"
    exit 1
fi

resolve_desktop_binary() {
    local requested="$1"
    local name="$2"
    if [[ -n "$requested" ]]; then
        printf '%s' "$requested"
        return 0
    fi
    for candidate in \
        "${REPO_ROOT}/target/debug/${name}" \
        "${REPO_ROOT}/target/release/${name}"; do
        if [[ -f "$candidate" ]]; then
            printf '%s' "$candidate"
            return 0
        fi
    done
    synveil_err "${name} artifact not found; pass --${name#synveil-}-binary=PATH"
    return 1
}

CLIENT_BINARY_SRC="$(resolve_desktop_binary "$CLIENT_BINARY_SRC" synveil-client)"
DESKTOP_BINARY_SRC="$(resolve_desktop_binary "$DESKTOP_BINARY_SRC" synveil-desktop)"

for binary_source in "$CLIENT_BINARY_SRC" "$DESKTOP_BINARY_SRC"; do
    if [[ ! -f "$binary_source" ]]; then
        synveil_err "binary source does not exist: $binary_source"
        exit 1
    fi
    if [[ "$binary_source" == *".."* ]]; then
        IFS='/' read -ra __parts <<< "$binary_source"
        for __p in "${__parts[@]}"; do
            if [[ "$__p" == ".." ]]; then
                synveil_err "binary path must not contain '..': $binary_source"
                exit 1
            fi
        done
    fi
done

# Validate no path escape for the binary itself
if [[ "$BINARY_SRC" == *".."* ]]; then
    # Check for ".." component in binary src
    IFS='/' read -ra __parts <<< "$BINARY_SRC"
    for __p in "${__parts[@]}"; do
        if [[ "$__p" == ".." ]]; then
            synveil_err "binary path must not contain '..': $BINARY_SRC"
            exit 1
        fi
    done
fi

synveil_log "installing Synveil scheduled-maintenance to staged root: $STAGED_ROOT"
synveil_log "binary source: $BINARY_SRC"
synveil_log "manifest: $MANIFEST_FILE"

# Failure injection counter (for upgrade-failure simulation)
FAIL_AFTER="${SYNVEIL_INSTALL_FAIL_AFTER:-}"
PACKAGE_DONE=0

# ---------------------------------------------------------------------------
# Per-entry installer (called by manifest iterator)
# ---------------------------------------------------------------------------
install_one() {
    local source="$1"
    local dest="$2"
    local mode="$3"
    local owner="$4"
    local group="$5"
    local class="$6"

    # Failure injection: check before processing next PACKAGE entry
    if [[ -n "$FAIL_AFTER" && "$class" == "PACKAGE" ]]; then
        if [[ "$PACKAGE_DONE" -ge "$FAIL_AFTER" ]]; then
            synveil_err "injected failure after $FAIL_AFTER PACKAGE artifacts (simulating mid-upgrade failure); config/state preserved, PACKAGE may be partially updated"
            return 1
        fi
    fi

    case "$class" in
        PACKAGE)
            synveil_dest_under_root "$STAGED_ROOT" "$dest" || return 1
            local src_path
            if [[ "$source" == "BINARY" ]]; then
                src_path="$BINARY_SRC"
            elif [[ "$source" == "BINARY_CLIENT" ]]; then
                src_path="$CLIENT_BINARY_SRC"
            elif [[ "$source" == "BINARY_DESKTOP" ]]; then
                src_path="$DESKTOP_BINARY_SRC"
            elif [[ "$source" == "-" ]]; then
                synveil_err "PACKAGE entry must have source file, got '-' for $dest"
                return 1
            else
                src_path="${REPO_ROOT}/${source}"
            fi
            synveil_log "install PACKAGE $dest <- $src_path ($mode $owner:$group)"
            synveil_atomic_install "$src_path" "${STAGED_ROOT%/}${dest}" "$mode" "$owner" "$group" || return 1
            PACKAGE_DONE=$((PACKAGE_DONE + 1))
            ;;
        CONFIG_DIRECTORY|CREDENTIAL_DIRECTORY)
            synveil_dest_under_root "$STAGED_ROOT" "$dest" || return 1
            local full="${STAGED_ROOT%/}${dest}"
            synveil_log "ensure $class $dest ($mode $owner:$group)"
            # mkdir -p with mode; only set mode/owner on the leaf, not recursively chown parent.
            mkdir -p "$full"
            chmod "$mode" "$full"
            if [[ "$(id -u)" -eq 0 ]]; then
                chown "${owner}:${group}" "$full" 2>/dev/null || synveil_log "chown $owner:$group $full failed in staging (user may not exist); mode preserved"
            else
                synveil_log "rootless staging: intended $owner:$group $mode for $dest (ownership validated via manifest, not chown)"
            fi
            # Do NOT create /etc/synveil/synveil-scheduled-maintenance.env or database-url secret here.
            # If admin already has files there, we preserve them. If not, we leave absent
            # (EnvironmentFile=- and LoadCredential handle absence via config error). Template lives at /usr/share.
            ;;
        STATE_DIRECTORY|RUNTIME_MANAGED)
            # Documented but NOT created by package payload; managed by tmpfiles.d / RuntimeDirectory.
            synveil_dest_under_root "$STAGED_ROOT" "$dest" || return 1
            synveil_log "skip $class $dest (managed by tmpfiles.d/RuntimeDirectory, not package payload)"
            ;;
        *)
            synveil_err "unknown manifest class: $class for $dest"
            return 1
            ;;
    esac
}

# Install in manifest order. Use a subshell to ensure failure propagates.
set +e
manifest_failed=0
while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [[ -z "$line" ]] && continue
    [[ "$line" == \#* ]] && continue
    read -r source dest mode owner group class <<< "$line"
    if ! install_one "$source" "$dest" "$mode" "$owner" "$group" "$class"; then
        manifest_failed=1
        break
    fi
done < "$MANIFEST_FILE"
set -e

if [[ "$manifest_failed" -ne 0 ]]; then
    synveil_err "installation failed (PACKAGE_DONE=$PACKAGE_DONE, FAIL_AFTER=${FAIL_AFTER:-none}); config/state/user-data remain untouched; PACKAGE may be partially updated — full rollback DEFERRED to distro packaging"
    exit 1
fi

synveil_log "install complete to $STAGED_ROOT (PACKAGE_DONE=$PACKAGE_DONE)"

# Post-install validation (non-fatal hints)
# Ensure no unexpected secret file was created.
if [[ -f "${STAGED_ROOT%/}/etc/synveil/synveil-scheduled-maintenance.env" ]]; then
    synveil_log "note: ${STAGED_ROOT}/etc/synveil/synveil-scheduled-maintenance.env exists (admin-created, preserved); fresh install would leave it absent and provide template at /usr/share/synveil/"
fi

# Document expected future steps (do NOT execute on staged root)
cat >&2 <<EOF
[synveil-install] next steps for real host (not executed in staged test):
  systemd-sysusers --root=$STAGED_ROOT (or systemd-sysusers)
  systemd-tmpfiles --create --root=$STAGED_ROOT (or --prefix=/var/lib/synveil)
  systemctl daemon-reload
  systemctl --user daemon-reload
  systemctl --user enable synveil-client.service  # explicit user autostart, never automatic here
  systemctl enable synveil-scheduled-maintenance.timer  # explicit enablement, not automatic in package-neutral layer
EOF

exit 0
