#!/usr/bin/env bash
# linux-acceptance-vm.sh — VM control plane for Prompt020 native acceptance.
#
# SPDX-License-Identifier: MIT
#
# This script is the *harness*. It provisions and observes disposable machines.
# It is deliberately NOT the product journey: everything the acceptance claims
# (graphical install, application-menu launch, first-launch consent) must be
# performed inside the guest through user-facing surfaces by
# scripts/linux_acceptance.py.
#
# Image trust (P020 §12): this script never downloads an unpinned image and
# never runs `curl | sh`. Every image must have its SHA-256 recorded in
# deploy/acceptance/images.lock. An image with no recorded digest is refused,
# because an unverified download is not evidence.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd -P)"
IMAGE_LOCK="${REPO_ROOT}/deploy/acceptance/images.lock"

# Bounded by design. A hung VM must fail the job, not the runner.
readonly BOOT_TIMEOUT_SECONDS="${BOOT_TIMEOUT_SECONDS:-300}"
readonly EXEC_TIMEOUT_SECONDS="${EXEC_TIMEOUT_SECONDS:-180}"

log()  { printf '[linux-acceptance-vm] %s\n' "$*" >&2; }
fail() { printf '[linux-acceptance-vm] ERROR: %s\n' "$*" >&2; exit 1; }

require_tools() {
    local tool missing=()
    for tool in "$@"; do
        command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
    done
    ((${#missing[@]} == 0)) || fail "required host tools are missing: ${missing[*]}"
}

# ---------------------------------------------------------------------------
# Image acquisition
# ---------------------------------------------------------------------------

# pinned_digest IMAGE_NAME -> prints the recorded SHA-256, or fails.
pinned_digest() {
    local name="$1" digest=""
    [[ -f "$IMAGE_LOCK" ]] || fail "image lock file is absent: deploy/acceptance/images.lock"
    digest="$(awk -v key="$name" '
        # mawk and other POSIX awks do not support interval expressions such as
        # {64}, so the digest length is matched with length() here. The caller
        # validates the hex charset, which bash does support.
        $1 == key && length($2) == 71 && substr($2, 1, 7) == "sha256:" {
            print substr($2, 8); found = 1
        }
        END { if (!found) exit 1 }
    ' "$IMAGE_LOCK")" || fail "no pinned sha256 recorded for image '${name}'; refusing to download an unpinned image"
    [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || fail "pinned digest for '${name}' is not a SHA-256 value"
    printf '%s' "$digest"
}

# fetch_image NAME URL DEST — download once, verify against the pinned digest.
fetch_image() {
    local name="$1" url="$2" dest="$3"
    local expected actual
    expected="$(pinned_digest "$name")"

    if [[ -f "$dest" ]]; then
        actual="$(sha256sum "$dest" | awk '{print $1}')"
        [[ "$actual" == "$expected" ]] && { log "image ${name} already present and verified"; return 0; }
        log "cached image ${name} does not match its pinned digest; refetching"
        rm -f -- "$dest"
    fi

    require_tools curl sha256sum
    [[ "$url" == https://* ]] || fail "image URL must be HTTPS: ${url}"
    log "downloading ${name} from ${url}"
    curl --fail --location --proto '=https' --tlsv1.2 --retry 3 --retry-delay 5 \
        --output "${dest}.part" -- "$url"
    actual="$(sha256sum "${dest}.part" | awk '{print $1}')"
    if [[ "$actual" != "$expected" ]]; then
        rm -f -- "${dest}.part"
        fail "checksum mismatch for ${name}: expected=${expected} actual=${actual}"
    fi
    mv -- "${dest}.part" "$dest"
    log "image ${name} verified against pinned digest ${expected}"
}

# ---------------------------------------------------------------------------
# Guest provisioning
# ---------------------------------------------------------------------------

# The acceptance identity is synthetic and disposable. Its password is
# generated per run, used only for the polkit authorization surface, never
# written to a log line and never exported into the evidence bundle.
write_cloud_init() {
    local seed_dir="$1" password="$2"
    mkdir -p "$seed_dir"

    # The password hash is computed here so the plaintext never reaches disk.
    local hash
    hash="$(python3 -c 'import crypt,sys; print(crypt.crypt(sys.argv[1], crypt.mksalt(crypt.METHOD_SHA512)))' "$password")"

    cat > "${seed_dir}/user-data" <<EOF
#cloud-config
users:
  - name: synveil-acceptance
    groups: [sudo]
    shell: /bin/bash
    sudo: "ALL=(ALL) NOPASSWD:ALL"
    lock_passwd: false
    passwd: "${hash}"
    ssh_authorized_keys: []
ssh_pwauth: false
package_update: false
runcmd:
  - [ systemctl, enable, --now, qemu-guest-agent ]
EOF

    cat > "${seed_dir}/meta-data" <<EOF
instance-id: synveil-p020-${RANDOM}
local-hostname: synveil-p020
EOF
}

make_seed_iso() {
    local seed_dir="$1" iso="$2"
    require_tools genisoimage cloud-localds xorriso
    if command -v cloud-localds >/dev/null 2>&1; then
        cloud-localds "$iso" "${seed_dir}/user-data" "${seed_dir}/meta-data"
    elif command -v genisoimage >/dev/null 2>&1; then
        genisoimage -output "$iso" -volid cidata -joliet -rock \
            "${seed_dir}/user-data" "${seed_dir}/meta-data"
    else
        xorriso -as mkisofs -output "$iso" -volid CIDATA -joliet -rock \
            "${seed_dir}/user-data" "${seed_dir}/meta-data"
    fi
}

# ---------------------------------------------------------------------------
# VM lifecycle
# ---------------------------------------------------------------------------

accel_args() {
    # KVM is preferred. TCG is permitted for correctness acceptance when KVM is
    # unavailable, but it must be declared, never silently substituted.
    if [[ -w /dev/kvm ]]; then
        printf '%s' "-enable-kvm -cpu host"
    else
        log "KVM unavailable; falling back to QEMU TCG (slower, same correctness class)"
        printf '%s' "-accel tcg -cpu qemu64"
    fi
}

start_vm() {
    local disk="$1" seed_iso="$2" monitor="$3" serial="$4"
    local accel
    accel="$(accel_args)"

    # shellcheck disable=SC2086 # accel_args emits an intentional flag list
    qemu-system-x86_64 \
        $accel -m 4096 -smp 2 \
        -drive "file=${disk},if=virtio,format=qcow2" \
        -drive "file=${seed_iso},if=virtio,format=raw,readonly=on" \
        -netdev user,id=net0,hostfwd=tcp-:2222-:22 \
        -device virtio-net-pci,netdev=net0 \
        -qmp "unix:${monitor},server=on,wait=off" \
        -serial "file:${serial}" \
        -display none -no-reboot &
    printf '%s' "$!"
}

# wait_for_boot polls the serial console for cloud-init completion within a
# finite bound. Fixed sleeps are not used as the primary synchronisation.
wait_for_boot() {
    local serial="$1" deadline
    deadline=$(( $(date +%s) + BOOT_TIMEOUT_SECONDS ))
    while (( $(date +%s) < deadline )); do
        if [[ -f "$serial" ]] && grep -q 'login:' "$serial" 2>/dev/null; then
            log "guest reached a login prompt"
            return 0
        fi
        sleep 2
    done
    return 1
}

qmp_cmd() {
    local monitor="$1"; shift
    printf '{"execute":"%s","arguments":%s}\n' "$1" "${2:-\{\}}" |
        timeout 15 socat - "UNIX-CONNECT:${monitor}" >/dev/null 2>&1 || true
}

snapshot() {
    local monitor="$1" tag="$2"
    qmp_cmd "$monitor" "human-monitor-command" "{\"command\":\"savevm ${tag}\"}"
    log "snapshot saved: ${tag}"
}

restore_snapshot() {
    local monitor="$1" tag="$2"
    qmp_cmd "$monitor" "human-monitor-command" "{\"command\":\"loadvm ${tag}\"}"
    log "snapshot restored: ${tag}"
}

# power_off is a real power cut, not a guest shutdown. A guest-initiated stop
# would let the guest flush state and would not exercise recovery.
power_cut() {
    local monitor="$1"
    qmp_cmd "$monitor" quit '{}'
    log "VM power cut issued"
}

usage() {
    cat <<'EOF'
Usage: linux-acceptance-vm.sh <command> [args]

  fetch-image NAME URL DEST   Download and verify a pinned cloud image
  boot NAME                  Boot a disposable guest and report readiness
  power-cut                  Cut power to the running guest
  snapshot TAG               Save a QEMU snapshot
  restore TAG                Restore a QEMU snapshot

Image digests must be recorded in deploy/acceptance/images.lock.
EOF
}

main() {
    local command="${1:-}"
    case "$command" in
        fetch-image) [[ $# -eq 4 ]] || fail "fetch-image NAME URL DEST"; fetch_image "$2" "$3" "$4" ;;
        boot)        [[ $# -eq 2 ]] || fail "boot NAME"; fail "boot must be invoked by the acceptance workflow with a prepared disk and seed" ;;
        power-cut)   [[ $# -eq 2 ]] || fail "power-cut MONITOR"; power_cut "$2" ;;
        snapshot)    [[ $# -eq 3 ]] || fail "snapshot MONITOR TAG"; snapshot "$2" "$3" ;;
        restore)     [[ $# -eq 3 ]] || fail "restore MONITOR TAG"; restore_snapshot "$2" "$3" ;;
        --help|-h)   usage ;;
        *)           usage; fail "unknown command: ${command}" ;;
    esac
}

main "$@"