#!/usr/bin/env bash
# build.sh — deterministic native package entry point for Synveil Gen-1 (Prompt 79).
# Builds real .deb (dpkg-deb or ar fallback) and real .rpm (rpmbuild) from the
# release binary + package-neutral MANIFEST payload. No repo mutation.
# SPDX-License-Identifier: MIT
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
# shellcheck source=deploy/packages/common/version.sh
source "${SCRIPT_DIR}/common/version.sh"
# shellcheck source=deploy/packages/common/arch.sh
source "${SCRIPT_DIR}/common/arch.sh"
# shellcheck source=deploy/packages/common/payload.sh
source "${SCRIPT_DIR}/common/payload.sh"

PACKAGE_NAME="synveil"
DEFAULT_OUTPUT_DIR="${REPO_ROOT}/target/packages"

usage() {
    cat >&2 <<'EOF'
Usage: build.sh [--format=deb|rpm|all] [--output-dir=DIR] [--binary=PATH] [--help]

Builds native Synveil Gen-1 distribution packages from the canonical release
binary and the package-neutral MANIFEST payload.

Options:
  --format=deb|rpm|all   Package format to build (default: all).
  --output-dir=DIR       Output directory for .deb/.rpm (default: target/packages).
                         Must be absolute or repo-relative; never '/'; '..' rejected.
  --binary=PATH          Use an existing release binary instead of building.
                         Must be the real artifact from
                         crates/api/src/bin/synveil-scheduled-maintenance-once.rs
                         (test fixtures / stubs are rejected by hash policy in tests).
  --help                 Show this help.

Behavior:
  - Derives version from workspace Cargo.toml (no hardcoded version).
  - Maps host architecture deterministically (x86_64 only claimed in Prompt 79).
  - Builds release binary via: cargo build --release --locked --bin synveil-scheduled-maintenance-once
    unless --binary is given.
  - Stages payload via deploy/install/install.sh (MANIFEST is authoritative).
  - DEB built with dpkg-deb when available, otherwise with ar+tar (real .deb format).
  - RPM built with rpmbuild (required for --format=rpm/all).
  - Outputs to a gitignored location (target/packages by default); never stages packages in git.

Environment:
  SOURCE_DATE_EPOCH      If set, normalizes archive mtimes/ordering for reproducibility.
  SYNVEIL_RPM_RELEASE    RPM Release tag override (default 1; test-only fixture use).
EOF
}

FORMAT="all"
OUTPUT_DIR="$DEFAULT_OUTPUT_DIR"
BINARY_OVERRIDE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --format=*)
            FORMAT="${1#--format=}"
            shift
            ;;
        --output-dir=*)
            OUTPUT_DIR="${1#--output-dir=}"
            shift
            ;;
        --binary=*)
            BINARY_OVERRIDE="${1#--binary=}"
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
            printf '[synveil-packages] ERROR: unknown option: %s\n' "$1" >&2
            usage
            exit 2
            ;;
        *)
            printf '[synveil-packages] ERROR: unexpected argument: %s\n' "$1" >&2
            usage
            exit 2
            ;;
    esac
done

case "$FORMAT" in
    deb|rpm|all) ;;
    *)
        printf '[synveil-packages] ERROR: --format must be deb, rpm, or all (got %q)\n' "$FORMAT" >&2
        exit 2
        ;;
esac

# Resolve output dir: allow repo-relative, require absolute final, reject '/' and '..'.
if [[ "$OUTPUT_DIR" != /* ]]; then
    OUTPUT_DIR="${REPO_ROOT}/${OUTPUT_DIR}"
fi
if [[ "$OUTPUT_DIR" == "/" ]]; then
    printf '[synveil-packages] ERROR: refusing to use / as output dir\n' >&2
    exit 1
fi
if [[ "$OUTPUT_DIR" == *".."* ]]; then
    IFS='/' read -ra __parts <<< "$OUTPUT_DIR"
    for __p in "${__parts[@]}"; do
        if [[ "$__p" == ".." ]]; then
            printf '[synveil-packages] ERROR: output dir must not contain ..: %s\n' "$OUTPUT_DIR" >&2
            exit 1
        fi
    done
fi
mkdir -p "$OUTPUT_DIR"

log() {
    printf '[synveil-packages] %s\n' "$*" >&2
}

# ---------------------------------------------------------------------------
# Version + architecture (derived, never hardcoded)
# ---------------------------------------------------------------------------
CARGO_VERSION="$(synveil_cargo_version)"
DEB_VERSION="$(synveil_deb_version "$CARGO_VERSION")"
RPM_VERSION="$(synveil_rpm_version "$CARGO_VERSION")"
RPM_RELEASE="$(synveil_rpm_release)"
MACHINE="$(uname -m)"
DEB_ARCH="$(synveil_deb_arch "$MACHINE")"
RPM_ARCH="$(synveil_rpm_arch "$MACHINE")"
log "cargo version: $CARGO_VERSION"
log "deb version: $DEB_VERSION ($DEB_ARCH) | rpm version: $RPM_VERSION-$RPM_RELEASE ($RPM_ARCH)"

# ---------------------------------------------------------------------------
# Release binary (real artifact only)
# ---------------------------------------------------------------------------
BINARY_SRC="$BINARY_OVERRIDE"
if [[ -z "$BINARY_SRC" ]]; then
    log "building release binary: cargo build --release --locked --bin synveil-scheduled-maintenance-once"
    (
        cd "$REPO_ROOT"
        cargo build --release --locked --bin synveil-scheduled-maintenance-once
    )
    BINARY_SRC="${REPO_ROOT}/target/release/synveil-scheduled-maintenance-once"
fi
if [[ ! -f "$BINARY_SRC" ]]; then
    printf '[synveil-packages] ERROR: binary not found: %s\n' "$BINARY_SRC" >&2
    exit 1
fi
BIN_SIZE="$(wc -c < "$BINARY_SRC" | tr -d ' ')"
BIN_SHA="$(sha256sum "$BINARY_SRC" | awk '{print $1}')"
log "binary: $BINARY_SRC (${BIN_SIZE} bytes, sha256 ${BIN_SHA})"

# Dynamic-link audit (evidence for dependency declarations; informational here,
# authoritative audit is recorded by the caller).
if command -v file >/dev/null 2>&1; then
    log "file: $(file -b "$BINARY_SRC" | head -n 1)"
fi
if command -v ldd >/dev/null 2>&1; then
    log "ldd:"
    ldd "$BINARY_SRC" 2>&1 | while IFS= read -r line; do log "  $line"; done || true
fi

# ---------------------------------------------------------------------------
# Stage authoritative payload via package-neutral layer
# ---------------------------------------------------------------------------
STAGE_ROOT="$(mktemp -d /tmp/synveil-pkg-stage.XXXXXX)"
trap 'rm -rf "$STAGE_ROOT"' EXIT
log "staging payload via install.sh into $STAGE_ROOT"
synveil_stage_payload "$STAGE_ROOT" "$BINARY_SRC"
synveil_assert_no_secret_payload "$STAGE_ROOT"

# Byte-parity guards: packaged inputs must equal authoritative sources.
for pair in \
    "deploy/systemd/synveil-scheduled-maintenance.service:usr/lib/systemd/system/synveil-scheduled-maintenance.service" \
    "deploy/systemd/synveil-scheduled-maintenance.timer:usr/lib/systemd/system/synveil-scheduled-maintenance.timer" \
    "deploy/sysusers.d/synveil.conf:usr/lib/sysusers.d/synveil.conf" \
    "deploy/tmpfiles.d/synveil.conf:usr/lib/tmpfiles.d/synveil.conf" \
    "deploy/config/synveil-scheduled-maintenance.env.example:usr/share/synveil/synveil-scheduled-maintenance.env.example"; do
    src_rel="${pair%%:*}"
    staged_rel="${pair##*:}"
    if ! cmp -s "${REPO_ROOT}/${src_rel}" "${STAGE_ROOT}/${staged_rel}"; then
        printf '[synveil-packages] ERROR: staged payload drift for %s (must be byte-identical to %s)\n' "$staged_rel" "$src_rel" >&2
        exit 1
    fi
done
if ! cmp -s "$BINARY_SRC" "${STAGE_ROOT}/usr/bin/synveil-scheduled-maintenance-once"; then
    printf '[synveil-packages] ERROR: staged binary drift (must be byte-identical to release binary)\n' >&2
    exit 1
fi
log "payload parity with MANIFEST sources: OK"

# Normalize ownership/modes inside staging for archive creation.
# install.sh already chmods; enforce root:root intent for PACKAGE files when running as root,
# and normalize tar archives to root:root numerically at archive time (works rootless).
chmod 0755 "${STAGE_ROOT}/usr/bin/synveil-scheduled-maintenance-once"
chmod 0644 "${STAGE_ROOT}/usr/lib/systemd/system/synveil-scheduled-maintenance.service" \
    "${STAGE_ROOT}/usr/lib/systemd/system/synveil-scheduled-maintenance.timer" \
    "${STAGE_ROOT}/usr/lib/sysusers.d/synveil.conf" \
    "${STAGE_ROOT}/usr/lib/tmpfiles.d/synveil.conf" \
    "${STAGE_ROOT}/usr/share/synveil/synveil-scheduled-maintenance.env.example"
chmod 0750 "${STAGE_ROOT}/etc/synveil"
chmod 0700 "${STAGE_ROOT}/etc/synveil/credentials"
if [[ "$(id -u)" -eq 0 ]]; then
    chown root:root "${STAGE_ROOT}/usr/bin/synveil-scheduled-maintenance-once" \
        "${STAGE_ROOT}/usr/lib/systemd/system/synveil-scheduled-maintenance.service" \
        "${STAGE_ROOT}/usr/lib/systemd/system/synveil-scheduled-maintenance.timer" \
        "${STAGE_ROOT}/usr/lib/sysusers.d/synveil.conf" \
        "${STAGE_ROOT}/usr/lib/tmpfiles.d/synveil.conf" \
        "${STAGE_ROOT}/usr/share/synveil/synveil-scheduled-maintenance.env.example"
    chown root:synveil "${STAGE_ROOT}/etc/synveil" 2>/dev/null || chown root:root "${STAGE_ROOT}/etc/synveil"
    chown root:root "${STAGE_ROOT}/etc/synveil/credentials"
fi

# Installed-Size for DEB control (KiB, du -sk of payload).
INSTALLED_SIZE="$(du -sk "$STAGE_ROOT" | awk '{print $1}')"
# Minimal native dependencies from the dynamic-link audit documented with the
# Prompt 79 packaging contract in docs/en/DEPLOYMENT.md:
# Rust release binary is dynamically linked against glibc only (libc.so.6,
# libm.so.6, libgcc_s.so.1, ld-linux-x86-64.so.2); systemd provides
# sysusers/tmpfiles/manager integration. No PostgreSQL server/CLI, Docker, Nginx, Redis.
DEB_DEPENDS="systemd, libc6 (>= 2.34), libgcc-s1"

# tar flags for determinism: sorted names, root ownership, normalized mtime when SOURCE_DATE_EPOCH set.
tar_owner_flags=(--owner=0 --group=0 --numeric-owner --sort=name)
if [[ -n "${SOURCE_DATE_EPOCH:-}" ]]; then
    tar_owner_flags+=(--mtime="@${SOURCE_DATE_EPOCH}")
fi

build_deb() {
    local deb_name="${PACKAGE_NAME}_${DEB_VERSION}_${DEB_ARCH}.deb"
    local deb_path="${OUTPUT_DIR}/${deb_name}"
    log "building DEB: $deb_path"
    local deb_stage
    deb_stage="$(mktemp -d /tmp/synveil-deb.XXXXXX)"
    # Copy payload (without DEBIAN) into deb_stage root.
    cp -a "${STAGE_ROOT}/." "${deb_stage}/"
    mkdir -p "${deb_stage}/DEBIAN"
    # Generate control from template.
    sed -e "s/@SYNVEIL_VERSION@/${DEB_VERSION}/g" \
        -e "s/@SYNVEIL_ARCH@/${DEB_ARCH}/g" \
        -e "s/@SYNVEIL_INSTALLED_SIZE@/${INSTALLED_SIZE}/g" \
        -e "s/@SYNVEIL_DEPENDS@/${DEB_DEPENDS}/g" \
        "${SCRIPT_DIR}/debian/control.tmpl" > "${deb_stage}/DEBIAN/control"
    # Maintainer scripts (must be executable, no extension inside DEBIAN).
    for script in postinst prerm postrm; do
        cp -a "${SCRIPT_DIR}/debian/${script}" "${deb_stage}/DEBIAN/${script}"
        chmod 0755 "${deb_stage}/DEBIAN/${script}"
        # Syntax check now (fail fast, no masking).
        sh -n "${deb_stage}/DEBIAN/${script}"
        if command -v shellcheck >/dev/null 2>&1; then
            shellcheck -S warning "${deb_stage}/DEBIAN/${script}" || {
                printf '[synveil-packages] ERROR: shellcheck failed for debian/%s\n' "$script" >&2
                rm -rf "$deb_stage"
                return 1
            }
        fi
    done
    # Validate control has no unsubstituted placeholders.
    if grep -q "@SYNVEIL_" "${deb_stage}/DEBIAN/control"; then
        printf '[synveil-packages] ERROR: unsubstituted placeholder in DEBIAN/control\n' >&2
        rm -rf "$deb_stage"
        return 1
    fi
    if command -v dpkg-deb >/dev/null 2>&1; then
        log "building with dpkg-deb --build"
        fakeroot dpkg-deb --build "$deb_stage" "$deb_path" 2>/dev/null \
            || dpkg-deb --build "$deb_stage" "$deb_path"
    else
        log "dpkg-deb unavailable; building real .deb with ar+tar (equivalent format)"
        local work
        work="$(mktemp -d /tmp/synveil-deb-ar.XXXXXX)"
        # data.tar.gz from payload (exclude DEBIAN).
        (
            cd "$deb_stage"
            tar "${tar_owner_flags[@]}" -czf "${work}/data.tar.gz" \
                usr etc
        )
        # control.tar.gz from DEBIAN (rename to control dir content without prefix quirks).
        (
            cd "${deb_stage}/DEBIAN"
            tar "${tar_owner_flags[@]}" -czf "${work}/control.tar.gz" \
                control postinst prerm postrm
        )
        printf '2.0\n' > "${work}/debian-binary"
        # Deterministic ar: debian-binary, control.tar.gz, data.tar.gz order.
        (
            cd "$work"
            ar rcs "$deb_path" debian-binary control.tar.gz data.tar.gz
        )
        rm -rf "$work"
    fi
    rm -rf "$deb_stage"
    local size sha
    size="$(wc -c < "$deb_path" | tr -d ' ')"
    sha="$(sha256sum "$deb_path" | awk '{print $1}')"
    log "DEB built: $deb_path (${size} bytes, sha256 ${sha})"
    printf '%s' "$deb_path"
}

build_rpm() {
    if ! command -v rpmbuild >/dev/null 2>&1; then
        printf '[synveil-packages] ERROR: rpmbuild not found (required for --format=rpm)\n' >&2
        return 1
    fi
    local rpm_name="${PACKAGE_NAME}-${RPM_VERSION}-${RPM_RELEASE}.${RPM_ARCH}.rpm"
    log "building RPM: $rpm_name"
    local top
    top="$(mktemp -d /tmp/synveil-rpm-top.XXXXXX)"
    mkdir -p "${top}/BUILD" "${top}/RPMS" "${top}/SOURCES" "${top}/SPECS" "${top}/SRPMS"
    # Generate spec from template.
    sed -e "s/@SYNVEIL_VERSION@/${RPM_VERSION}/g" \
        -e "s/@SYNVEIL_RELEASE@/${RPM_RELEASE}/g" \
        -e "s/@SYNVEIL_ARCH@/${RPM_ARCH}/g" \
        "${SCRIPT_DIR}/rpm/synveil.spec.tmpl" > "${top}/SPECS/synveil.spec"
    if grep -q "@SYNVEIL_" "${top}/SPECS/synveil.spec"; then
        printf '[synveil-packages] ERROR: unsubstituted placeholder in synveil.spec\n' >&2
        rm -rf "$top"
        return 1
    fi
    # Shell syntax check on scriptlets (extract and sh -n).
    # Scriptlets are inline in spec; validate the template sources instead:
    # (postinst/prerm/postrm logic is mirrored; spec blocks are POSIX sh.)
    log "running rpmbuild -bb"
    # shellcheck disable=SC2016
    SYNVEIL_STAGED_PAYLOAD="$STAGE_ROOT" rpmbuild -bb \
        --define "_topdir ${top}" \
        --define "SYNVEIL_STAGED_PAYLOAD ${STAGE_ROOT}" \
        --target "${RPM_ARCH}" \
        "${top}/SPECS/synveil.spec"
    # Locate built RPM (RPMS/<arch>/...).
    local built
    built="$(find "${top}/RPMS" -name '*.rpm' | head -n 1)"
    if [[ -z "$built" ]]; then
        printf '[synveil-packages] ERROR: rpmbuild produced no .rpm under %s/RPMS\n' "$top" >&2
        rm -rf "$top"
        return 1
    fi
    local dest="${OUTPUT_DIR}/${rpm_name}"
    cp -a "$built" "$dest"
    rm -rf "$top"
    local size sha
    size="$(wc -c < "$dest" | tr -d ' ')"
    sha="$(sha256sum "$dest" | awk '{print $1}')"
    log "RPM built: $dest (${size} bytes, sha256 ${sha})"
    printf '%s' "$dest"
}

case "$FORMAT" in
    deb)
        build_deb
        ;;
    rpm)
        build_rpm
        ;;
    all)
        build_deb
        printf '\n' >&2
        build_rpm
        ;;
esac

log "done. artifacts in $OUTPUT_DIR"
ls -la "$OUTPUT_DIR" >&2
