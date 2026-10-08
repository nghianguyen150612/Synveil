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
readonly VM_STATE_DIR="${SYNVEIL_VM_STATE_DIR:-${TMPDIR:-/tmp}/synveil-p020-vm}"

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
    local seed_dir="$1" password="$2" public_key="$3" platform="$4"
    mkdir -p "$seed_dir"

    # The password hash is computed here so the plaintext never reaches disk.
    local hash
    hash="$(python3 -c 'import crypt,sys; print(crypt.crypt(sys.argv[1], crypt.mksalt(crypt.METHOD_SHA512)))' "$password")"

    local setup_script admin_group
    case "$platform" in
        ubuntu)
            admin_group=sudo
            setup_script='apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y ubuntu-desktop-minimal gnome-software gnome-screenshot policykit-1 at-spi2-core xdotool dbus-x11 libsecret-tools qemu-guest-agent && mkdir -p /etc/gdm3 && printf "[daemon]\\nAutomaticLoginEnable=true\\nAutomaticLogin=synveil-acceptance\\nWaylandEnable=false\\n" > /etc/gdm3/custom.conf && systemctl enable gdm3'
            ;;
        fedora)
            admin_group=wheel
            setup_script='dnf -y group install "Fedora Workstation" && dnf -y install gnome-software gnome-screenshot at-spi2-core xdotool dbus-x11 libsecret qemu-guest-agent && mkdir -p /etc/gdm && printf "[daemon]\\nAutomaticLoginEnable=True\\nAutomaticLogin=synveil-acceptance\\nWaylandEnable=false\\n" > /etc/gdm/custom.conf && systemctl enable gdm'
            ;;
        *)
            fail "unsupported guest platform for cloud-init: ${platform}"
            ;;
    esac

    cat > "${seed_dir}/user-data" <<EOF
#cloud-config
users:
  - name: synveil-acceptance
    groups: [${admin_group}]
    shell: /bin/bash
    sudo: "ALL=(ALL) ALL"
    lock_passwd: false
    passwd: "${hash}"
    ssh_authorized_keys:
      - ${public_key}
ssh_pwauth: false
package_update: false
runcmd:
  - [ bash, -lc, ${setup_script@Q} ]
  - [ bash, -lc, "mkdir -p /var/lib/synveil-acceptance && touch /var/lib/synveil-acceptance/desktop-ready" ]
  - [ systemctl, set-default, graphical.target ]
  - [ systemctl, enable, --now, qemu-guest-agent ]
EOF

    cat > "${seed_dir}/meta-data" <<EOF
instance-id: synveil-p020-${RANDOM}
local-hostname: synveil-p020
EOF
}

write_vm_metadata() {
    local name="$1" pid="$2" monitor="$3" serial="$4" ssh_port="$5" key="$6" disk="$7"
    mkdir -p "$VM_STATE_DIR"
    cat >"${VM_STATE_DIR}/${name}.env" <<EOF
pid=${pid}
monitor=${monitor}
serial=${serial}
ssh_port=${ssh_port}
key=${key}
disk=${disk}
EOF
}

load_vm_metadata() {
    local name="$1"
    local metadata="${VM_STATE_DIR}/${name}.env"
    [[ -f "$metadata" ]] || fail "VM metadata is absent: ${metadata}"
    # This file is emitted by this script and contains only fixed path/value
    # fields; scenario JSON never reaches this source/eval boundary.
    # shellcheck disable=SC1090
    source "$metadata"
}

make_seed_iso() {
    local seed_dir="$1" iso="$2"
    if command -v cloud-localds >/dev/null 2>&1; then
        cloud-localds "$iso" "${seed_dir}/user-data" "${seed_dir}/meta-data"
    elif command -v genisoimage >/dev/null 2>&1; then
        genisoimage -output "$iso" -volid cidata -joliet -rock \
            "${seed_dir}/user-data" "${seed_dir}/meta-data"
    else
        require_tools xorriso
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
    local disk="$1" seed_iso="$2" monitor="$3" serial="$4" ssh_port="$5"
    local accel
    accel="$(accel_args)"

    # shellcheck disable=SC2086 # accel_args emits an intentional flag list
    qemu-system-x86_64 \
        $accel -m 4096 -smp 2 \
        -drive "file=${disk},if=virtio,format=qcow2" \
        -drive "file=${seed_iso},if=virtio,format=raw,readonly=on" \
        -netdev user,id=net0,hostfwd="127.0.0.1:${ssh_port}-:22" \
        -device virtio-net-pci,netdev=net0 \
        -qmp "unix:${monitor},server=on,wait=off" \
        -serial "file:${serial}" \
        -display none -vga virtio -no-reboot \
        </dev/null >"${serial}.qemu.log" 2>&1 &
    printf '%s' "$!"
}

wait_for_ssh() {
    local port="$1" key="$2" deadline
    deadline=$(( $(date +%s) + BOOT_TIMEOUT_SECONDS ))
    while (( $(date +%s) < deadline )); do
        if ssh -i "$key" -o BatchMode=yes -o ConnectTimeout=5 \
            -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
            -p "$port" synveil-acceptance@127.0.0.1 true >/dev/null 2>&1; then
            log "guest SSH is ready on port ${port}"
            return 0
        fi
        sleep 2
    done
    return 1
}

wait_for_guest_readiness() {
    local port="$1" key="$2" deadline
    deadline=$(( $(date +%s) + BOOT_TIMEOUT_SECONDS ))
    while (( $(date +%s) < deadline )); do
        if guest_exec "$port" "$key" readiness >/dev/null 2>&1; then
            log "guest graphical provisioning is ready"
            return 0
        fi
        sleep 5
    done
    return 1
}

record_boot_diagnostics() {
    local name="$1" serial="$2" stage="$3"
    printf '%s\n' "$stage" >"${VM_STATE_DIR}/${name}.boot-status"
    log "guest boot did not reach readiness: ${stage}"
    for file in "$serial" "${serial}.qemu.log"; do
        if [[ -f "$file" ]]; then
            log "last 120 lines of ${file}"
            tail -n 120 "$file" >&2
        fi
    done
}

guest_exec() {
    local port="$1" key="$2" command_name="$3"
    case "$command_name" in
        readiness)
            ssh -i "$key" -o BatchMode=yes -o StrictHostKeyChecking=no \
                -o UserKnownHostsFile=/dev/null -p "$port" synveil-acceptance@127.0.0.1 \
                'test -f /var/lib/synveil-acceptance/desktop-ready'
            ;;
        facts)
            ssh -i "$key" -o BatchMode=yes -o StrictHostKeyChecking=no \
                -o UserKnownHostsFile=/dev/null -p "$port" synveil-acceptance@127.0.0.1 \
                'cat /etc/os-release; printf "ARCH=%s\\n" "$(uname -m)"; printf "KERNEL=%s\\n" "$(uname -r)"; printf "SESSION=%s\\n" "${XDG_SESSION_TYPE:-unknown}"'
            ;;
        screenshot)
            local destination="$4"
            ssh -i "$key" -o BatchMode=yes -o StrictHostKeyChecking=no \
                -o UserKnownHostsFile=/dev/null -p "$port" synveil-acceptance@127.0.0.1 \
                'DISPLAY=:0 gnome-screenshot -f /tmp/synveil-acceptance-failure.png'
            scp -q -i "$key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
                -P "$port" synveil-acceptance@127.0.0.1:/tmp/synveil-acceptance-failure.png "$destination"
            ;;
        run-scenario)
            local scenario="$4" artifact_type="$5" evidence="$6"
            case "$scenario" in
                INSTALL-JOURNEY-[2-8]|FIRST-RUN-[1-2]|UPGRADE-TEMPLATE-1) ;;
                *) fail "scenario is not in the reviewed acceptance vocabulary: ${scenario}" ;;
            esac
            case "$artifact_type" in
                AUTO|DEB|RPM|appimage|APPIMAGE) ;;
                *) fail "artifact type is not in the reviewed acceptance vocabulary: ${artifact_type}" ;;
            esac
            case "$evidence" in
                native-clean-machine|interruption-power-cycle) ;;
                *) fail "evidence class is not in the reviewed acceptance vocabulary: ${evidence}" ;;
            esac
            ssh -i "$key" -o BatchMode=yes -o StrictHostKeyChecking=no \
                -o UserKnownHostsFile=/dev/null -p "$port" synveil-acceptance@127.0.0.1 \
                "DISPLAY=:0 XDG_SESSION_TYPE=x11 XDG_RUNTIME_DIR=/run/user/\$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$(id -u)/bus python3 /home/synveil-acceptance/p020/scripts/linux_acceptance.py run ${scenario@Q} --manifest /home/synveil-acceptance/p020/target/packages/SYNVEIL-RELEASE-MANIFEST.json --artifact-type ${artifact_type@Q} --evidence ${evidence@Q}"
            ;;
        *)
            fail "unsupported guest-control command: ${command_name}"
            ;;
    esac
}

stage_acceptance() {
    local name="$1" root="$2"
    load_vm_metadata "$name"
    [[ -d "$root/scripts" && -d "$root/tests/install-acceptance" && -d "$root/target/packages" ]] ||
        fail "acceptance staging root is incomplete: ${root}"
    ssh -i "$key" -o BatchMode=yes -o StrictHostKeyChecking=no \
        -o UserKnownHostsFile=/dev/null -p "$ssh_port" synveil-acceptance@127.0.0.1 \
        'mkdir -p /home/synveil-acceptance/p020/scripts /home/synveil-acceptance/p020/tests /home/synveil-acceptance/p020/target/packages'
    scp -q -r -i "$key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        -P "$ssh_port" "$root/scripts/linux_acceptance.py" "$root/scripts/install_acceptance.py" \
        synveil-acceptance@127.0.0.1:/home/synveil-acceptance/p020/scripts/
    scp -q -r -i "$key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        -P "$ssh_port" "$root/tests/install-acceptance" \
        synveil-acceptance@127.0.0.1:/home/synveil-acceptance/p020/tests/
    scp -q -r -i "$key" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        -P "$ssh_port" "$root/target/packages"/* \
        synveil-acceptance@127.0.0.1:/home/synveil-acceptance/p020/target/packages/
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
    # A brace inside a parameter-expansion default closes it early and appends
    # an extra '}' to supplied JSON. Keep the empty object outside the expansion.
    local execute="$1" arguments="${2:-}"
    [[ -n "$arguments" ]] || arguments='{}'
    python3 - "$monitor" "$execute" "$arguments" <<'PY'
import json
import socket
import sys

monitor, execute, arguments = sys.argv[1:]
request = {"execute": execute, "arguments": json.loads(arguments)}
with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as channel:
    channel.settimeout(15)
    channel.connect(monitor)
    greeting = channel.recv(65536)
    if not greeting:
        raise SystemExit("QMP did not provide its greeting")
    channel.sendall(b'{"execute":"qmp_capabilities"}\r\n')
    channel.recv(65536)
    channel.sendall((json.dumps(request) + "\r\n").encode())
    response = channel.recv(65536)
    if b'"error"' in response:
        raise SystemExit(response.decode(errors="replace"))
PY
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
    local name="$1"
    load_vm_metadata "$name"
    kill -KILL "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    log "VM power cut issued for ${name}"
}

usage() {
    cat <<'EOF'
Usage: linux-acceptance-vm.sh <command> [args]

  fetch-image NAME URL DEST   Download and verify a pinned cloud image
  prepare NAME IMAGE PLATFORM KEY DEST
                            Create a clean overlay and cloud-init seed
  boot NAME DISK SEED MONITOR SERIAL SSH_PORT KEY
                            Boot and wait for the guest control plane
  guest-exec NAME COMMAND [ARGS]
                            Run one fixed guest-control probe or scenario
  stage NAME REPO_ROOT      Stage reviewed acceptance files and artifacts
  power-cut NAME             Force-stop the VM process without guest shutdown
  snapshot TAG               Save a QEMU snapshot
  restore NAME TAG           Restore a QEMU snapshot

Image digests must be recorded in deploy/acceptance/images.lock.
EOF
}

main() {
    local command="${1:-}"
    case "$command" in
        fetch-image) [[ $# -eq 4 ]] || fail "fetch-image NAME URL DEST"; fetch_image "$2" "$3" "$4" ;;
        prepare)
            [[ $# -eq 6 ]] || fail "prepare NAME IMAGE PLATFORM KEY DEST"
            require_tools qemu-img python3
            name="$2"; image="$3"; platform="$4"; key="$5"; destination="$6"
            [[ -f "$image" ]] || fail "guest image is absent: ${image}"
            mkdir -p "$destination"
            qemu-img create -f qcow2 -F qcow2 -b "$image" "${destination}/${name}.qcow2" >/dev/null
            password="${SYNVEIL_ACCEPTANCE_PASSWORD:-$(python3 -c 'import secrets; print(secrets.token_urlsafe(18))')}"
            write_cloud_init "${destination}/seed" "$password" "$(cat "${key}.pub")" "$platform"
            make_seed_iso "${destination}/seed" "${destination}/${name}-seed.iso"
            printf '%s\n' "$password" >"${destination}/${name}.password"
            chmod 600 "${destination}/${name}.password"
            ;;
        boot)
            [[ $# -eq 8 ]] || fail "boot NAME DISK SEED MONITOR SERIAL SSH_PORT KEY"
            require_tools qemu-system-x86_64 ssh
            name="$2"; disk="$3"; seed="$4"; monitor="$5"; serial="$6"; ssh_port="$7"; key="$8"
            mkdir -p "$(dirname "$monitor")" "$(dirname "$serial")"
            pid="$(start_vm "$disk" "$seed" "$monitor" "$serial" "$ssh_port")"
            write_vm_metadata "$name" "$pid" "$monitor" "$serial" "$ssh_port" "$key" "$disk"
            if ! wait_for_ssh "$ssh_port" "$key"; then
                record_boot_diagnostics "$name" "$serial" "ssh-timeout"
                return 1
            fi
            if ! wait_for_guest_readiness "$ssh_port" "$key"; then
                record_boot_diagnostics "$name" "$serial" "readiness-timeout"
                return 1
            fi
            printf '%s\n' ready >"${VM_STATE_DIR}/${name}.boot-status"
            ;;
        guest-exec)
            [[ $# -ge 3 && $# -le 7 ]] || fail "guest-exec NAME COMMAND [ARGS]"
            require_tools ssh scp timeout
            load_vm_metadata "$2"
            timeout "$EXEC_TIMEOUT_SECONDS" "$0" _guest-exec-loaded "$2" "$3" "${4:-}" "${5:-}" "${6:-}" "${7:-}"
            ;;
        _guest-exec-loaded)
            [[ $# -ge 3 && $# -le 7 ]] || fail "internal guest-control invocation"
            load_vm_metadata "$2"
            guest_exec "$ssh_port" "$key" "$3" "${4:-}" "${5:-}" "${6:-}" "${7:-}"
            ;;
        stage)
            [[ $# -eq 3 ]] || fail "stage NAME REPO_ROOT"
            require_tools ssh scp timeout
            timeout "$EXEC_TIMEOUT_SECONDS" "$0" _stage "$2" "$3"
            ;;
        _stage)      [[ $# -eq 3 ]] || fail "internal staging invocation"; stage_acceptance "$2" "$3" ;;
        power-cut)   [[ $# -eq 2 ]] || fail "power-cut NAME"; power_cut "$2" ;;
        snapshot)    [[ $# -eq 3 ]] || fail "snapshot NAME TAG"; load_vm_metadata "$2"; snapshot "$monitor" "$3" ;;
        restore)     [[ $# -eq 3 ]] || fail "restore NAME TAG"; load_vm_metadata "$2"; restore_snapshot "$monitor" "$3" ;;
        --help|-h)   usage ;;
        *)           usage; fail "unknown command: ${command}" ;;
    esac
}

main "$@"
