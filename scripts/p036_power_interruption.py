#!/usr/bin/env python3
"""Hard-stop a marked disposable libvirt VM and verify Host state after boot."""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import time
import uuid
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
MARKER = Path("/etc/synveil-p036-disposable")
STABLE = (
    "server_installation_id", "storage_id", "storage_root_identity",
    "network_integration_id", "postgres_system_identifier", "postgres_data_identity",
    "postgres_runtime_identity", "administrator_id", "library_id",
    "credential_sha256", "server_objects_sha256", "managed_ca_sha256",
    "postgres_private", "api_loopback", "services_ready", "https_ready",
)


class EnvironmentBlocked(RuntimeError):
    pass


class AcceptanceFailed(RuntimeError):
    pass


def command(argv: list[str], *, timeout: int = 30, input_text: str | None = None,
            check: bool = True) -> subprocess.CompletedProcess[str]:
    try:
        result = subprocess.run(
            argv, input=input_text, text=True, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, timeout=timeout, check=False, shell=False,
            env={"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LC_ALL": "C", "LANG": "C"},
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise AcceptanceFailed(f"bounded control command failed: {Path(argv[0]).name}") from error
    if len(result.stdout) > 1_000_000:
        raise AcceptanceFailed("bounded control command output exceeded limit")
    if check and result.returncode != 0:
        raise AcceptanceFailed(f"control command failed: {Path(argv[0]).name}")
    return result


def required_path(name: str, private: bool) -> Path:
    value = os.environ.get(name, "")
    if not value:
        raise EnvironmentBlocked(f"required VM control input {name} is not configured")
    path = Path(value)
    try:
        info = path.lstat()
    except OSError as error:
        raise EnvironmentBlocked(f"required VM control file for {name} is unavailable") from error
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise EnvironmentBlocked(f"required VM control file for {name} is not a regular file")
    if private and (info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) not in {0o400, 0o600}):
        raise EnvironmentBlocked(f"SSH identity for {name} must be user-owned and mode 0400 or 0600")
    if not private and (info.st_uid != os.getuid() or info.st_mode & 0o022):
        raise EnvironmentBlocked(f"known_hosts file for {name} must be user-owned and not writable by others")
    return path.resolve(strict=True)


def domain_identity(name: str, expected_uuid: str) -> None:
    result = command(["virsh", "dumpxml", name])
    try:
        root = ET.fromstring(result.stdout)
    except ET.ParseError as error:
        raise AcceptanceFailed("libvirt returned malformed disposable VM metadata") from error
    actual_uuid = root.findtext("uuid", "").strip().lower()
    description = root.findtext("description", "").strip()
    if actual_uuid != expected_uuid:
        raise EnvironmentBlocked("libvirt domain UUID differs from the authorized P036 VM UUID")
    if description != "synveil-p036-disposable-v1":
        raise EnvironmentBlocked("libvirt VM does not carry the required disposable P036 marker")
    allowed_roots = [Path("/var/lib/libvirt/images"), Path("/var/lib/synveil-p036-vm")]
    try:
        roots = [path.resolve(strict=True) for path in allowed_roots]
    except OSError as error:
        raise EnvironmentBlocked("approved disposable VM disk roots are unavailable") from error
    sources = [node.get("file") or "" for node in root.findall(".//devices/disk/source")]
    if not sources:
        raise EnvironmentBlocked("P036 VM has no file-backed disposable disks")
    for source in sources:
        path = Path(source)
        try:
            info = path.lstat()
            resolved = path.resolve(strict=True)
        except OSError as error:
            raise EnvironmentBlocked("P036 VM disk source is unavailable") from error
        if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
                or resolved != path or not any(resolved.parent == base or base in resolved.parents for base in roots)):
            raise EnvironmentBlocked("P036 VM disks are not confined, single-link files in approved disposable image roots")


def ssh_base(identity: Path, known_hosts: Path, target: str) -> list[str]:
    return [
        "ssh", "-i", str(identity), "-o", "BatchMode=yes", "-o", "IdentitiesOnly=yes",
        "-o", "StrictHostKeyChecking=yes", "-o", f"UserKnownHostsFile={known_hosts}",
        "-o", "ConnectTimeout=8", target,
        "sudo -n /usr/bin/python3 -",
    ]


def snapshot(args: list[str], source: str, *, timeout: int = 30) -> dict:
    result = command(args, timeout=timeout, input_text=source)
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise AcceptanceFailed("guest preservation snapshot was not valid JSON") from error
    if value.get("evidence") != "PASS_NATIVE":
        raise AcceptanceFailed("guest snapshot did not assert native readiness")
    return value


def compare(before: dict, after: dict) -> None:
    changed = [key for key in STABLE if before.get(key) != after.get(key)]
    if changed:
        raise AcceptanceFailed("power recovery changed durable Host state: " + ", ".join(changed))


def main() -> int:
    if os.geteuid() == 0:
        raise EnvironmentBlocked("run VM control from the unprivileged P036 runner account")
    try:
        marker = MARKER.lstat()
        marker_bytes = MARKER.read_bytes()
    except OSError as error:
        raise EnvironmentBlocked("root-owned P036 disposable-runner marker is absent") from error
    if (not stat.S_ISREG(marker.st_mode) or marker.st_uid != 0 or marker.st_nlink != 1
            or stat.S_IMODE(marker.st_mode) != 0o400
            or marker_bytes != b"synveil-p036-disposable-v1\n"):
        raise EnvironmentBlocked("P036 runner marker does not match the disposable VM contract")
    virsh = shutil.which("virsh")
    ssh = shutil.which("ssh")
    if virsh is None or ssh is None:
        raise EnvironmentBlocked("virsh and OpenSSH are required on the P036 VM-control runner")

    name = os.environ.get("SYNVEIL_P036_VM_NAME", "")
    if not re.fullmatch(r"synveil-p036-[a-z0-9-]{1,48}", name):
        raise EnvironmentBlocked("SYNVEIL_P036_VM_NAME must name a marked synveil-p036 disposable VM")
    try:
        expected_uuid = str(uuid.UUID(os.environ.get("SYNVEIL_P036_VM_UUID", "")))
    except ValueError as error:
        raise EnvironmentBlocked("SYNVEIL_P036_VM_UUID must be a canonical libvirt UUID") from error
    domain_identity(name, expected_uuid)
    identity = required_path("SYNVEIL_P036_VM_SSH_IDENTITY", private=True)
    known_hosts = required_path("SYNVEIL_P036_VM_KNOWN_HOSTS", private=False)
    host = os.environ.get("SYNVEIL_P036_VM_SSH_HOST", "")
    user = os.environ.get("SYNVEIL_P036_VM_SSH_USER", "")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}", host):
        raise EnvironmentBlocked("SYNVEIL_P036_VM_SSH_HOST must be a pinned guest address or hostname")
    if not re.fullmatch(r"[a-z_][a-z0-9_-]{0,31}", user):
        raise EnvironmentBlocked("SYNVEIL_P036_VM_SSH_USER must be an unprivileged guest account")
    target = f"{user}@{host}"
    ssh_command = ssh_base(identity, known_hosts, target)
    snapshot_program = (ROOT / "scripts/p036-snapshot-state.py").read_text(encoding="utf-8")

    state = command(["virsh", "domstate", expected_uuid]).stdout.strip().lower()
    if state != "running":
        raise EnvironmentBlocked("P036 guest must be running with a completed Host setup")
    before = snapshot(ssh_command, snapshot_program)

    # `virsh destroy` is an abrupt power cut. The VM UUID, signed-in-purpose
    # marker and confined disk sources were checked immediately beforehand.
    domain_identity(name, expected_uuid)
    command(["virsh", "destroy", "--domain", expected_uuid], timeout=60)
    if command(["virsh", "domstate", expected_uuid]).stdout.strip().lower() != "shut off":
        raise AcceptanceFailed("P036 VM did not reach the shut-off state after the interruption")
    command(["virsh", "start", "--domain", expected_uuid], timeout=60)

    deadline = time.monotonic() + 360
    after = None
    while time.monotonic() < deadline:
        domain_identity(name, expected_uuid)
        try:
            after = snapshot(ssh_command, snapshot_program, timeout=25)
            break
        except AcceptanceFailed:
            time.sleep(3)
    if after is None:
        raise AcceptanceFailed("guest did not boot to authoritative Host readiness within six minutes")
    compare(before, after)
    print(json.dumps({
        "evidence": "PASS_NATIVE",
        "scenario": "hard_power_interruption_and_recovery",
        "vm_uuid": expected_uuid,
        "server_installation_id": after["server_installation_id"],
        "postgres_system_identifier": after["postgres_system_identifier"],
        "storage_id": after["storage_id"],
        "preserved_state": True,
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except EnvironmentBlocked as error:
        print(f"BLOCKED_ENVIRONMENT: {error}", file=sys.stderr)
        raise SystemExit(2)
    except AcceptanceFailed as error:
        print(f"FAILED: {error}", file=sys.stderr)
        raise SystemExit(1)
