#!/usr/bin/env python3
"""Run P036 on a provisioned, disposable Ubuntu/Fedora native runner."""
from __future__ import annotations

import json
import os
from pathlib import Path
import platform
from pathlib import PurePosixPath
import shutil
import stat
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
DISPOSABLE_MARKER = Path("/etc/synveil-p036-disposable")
MAX_OUTPUT = 1_000_000
MAX_ARTIFACT_BYTES = 2 * 1024 * 1024 * 1024


class BlockedEnvironment(RuntimeError):
    pass


class BlockedExternalInput(RuntimeError):
    pass


class AcceptanceFailure(RuntimeError):
    pass


def run(argv: list[str], *, timeout: int = 120, input_text: str | None = None,
        env: dict[str, str] | None = None, check: bool = True) -> subprocess.CompletedProcess[str]:
    try:
        result = subprocess.run(
            argv, input=input_text, text=True, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, timeout=timeout, check=False, shell=False, env=env,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise AcceptanceFailure(f"bounded command failed: {Path(argv[0]).name}") from error
    if len(result.stdout) + len(result.stderr) > MAX_OUTPUT:
        raise AcceptanceFailure(f"bounded command output exceeded limit: {Path(argv[0]).name}")
    if check and result.returncode:
        detail = result.stderr.strip().splitlines()[-1:] or result.stdout.strip().splitlines()[-1:]
        raise AcceptanceFailure(f"{Path(argv[0]).name} failed" + (f": {detail[0][:400]}" if detail else ""))
    return result


def root_run(argv: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    sudo = shutil.which("sudo")
    if sudo is None:
        raise BlockedEnvironment("sudo is unavailable")
    return run([sudo, "-n", *argv], **kwargs)


def check_disposable_runner() -> None:
    if os.geteuid() == 0:
        raise BlockedEnvironment("run the Host journey as an unprivileged desktop account")
    try:
        info = DISPOSABLE_MARKER.lstat()
        value = DISPOSABLE_MARKER.read_bytes()
    except OSError as error:
        raise BlockedEnvironment("root-owned disposable-runner marker is absent") from error
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
            or stat.S_IMODE(info.st_mode) != 0o400
            or value != b"synveil-p036-disposable-v1\n"):
        raise BlockedEnvironment("disposable-runner marker does not match the P036 contract")


def check_authorization_session() -> None:
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS"):
        raise BlockedEnvironment("P036 Host authorization requires an active desktop D-Bus session")
    if not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")):
        raise BlockedEnvironment("P036 Host authorization requires an active graphical login session")
    if shutil.which("pkexec") is None:
        raise BlockedEnvironment("polkit pkexec is unavailable for normal Host authorization")


def host_platform() -> str:
    fields = {}
    try:
        for line in Path("/etc/os-release").read_text(encoding="utf-8").splitlines():
            key, sep, value = line.partition("=")
            if sep:
                fields[key] = value.strip().strip('"')
    except OSError as error:
        raise BlockedEnvironment("cannot inspect native runner OS identity") from error
    found = {("ubuntu", "24.04"): "ubuntu-24.04", ("fedora", "42"): "fedora-42"}.get(
        (fields.get("ID"), fields.get("VERSION_ID"))
    )
    if found is None or found != sys.argv[1]:
        raise BlockedEnvironment("native runner OS does not match the requested P036 platform")
    machine = platform.machine().lower()
    if machine not in {"x86_64", "amd64"}:
        raise BlockedEnvironment("P036 native acceptance requires x86_64")
    if not Path("/run/systemd/system").is_dir():
        raise BlockedEnvironment("the runner is not booted under the systemd system manager")
    return found


def require_clean_host() -> None:
    # This journey has no reset mode. Existing or ambiguous state is preserved
    # and sent to the explicit repair lane instead of being removed.
    paths = (
        Path("/etc/synveil/server-config.json"),
        Path("/etc/synveil/host-setup.json"),
        Path("/etc/synveil/credentials"),
        Path("/etc/synveil/network"),
        Path("/etc/synveil/postgresql"),
        Path("/var/lib/synveil/postgresql/17/data"),
        Path("/var/lib/synveil/postgresql/17"),
        Path("/var/lib/synveil/storage"),
        Path("/etc/synveil/host-runtime-catalog.json"),
        Path("/opt/synveil/postgresql/17"),
        Path("/opt/synveil/server"),
        Path("/opt/synveil/caddy"),
    )
    if any(path.exists() or path.is_symlink() for path in paths):
        raise BlockedEnvironment(
            "runner contains existing or ambiguous Host state; preserve it and provision a fresh P036 runner"
        )
    for unit in (
        "synveil-server.target", "synveil-postgresql.service",
        "synveil-server-migrate.service", "synveil-api.service",
        "synveil-worker.service", "synveil-edge.service",
    ):
        result = run(["/usr/bin/systemctl", "show", unit, "--property=LoadState", "--value"], check=False)
        if result.returncode == 0 and result.stdout.strip() not in {"not-found", ""}:
            raise BlockedEnvironment("Synveil systemd units already exist on the runner")


def unique_json_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise AcceptanceFailure("duplicate P036 inventory JSON key")
        result[key] = value
    return result


def copy_confined_input(source_root: Path, destination_root: Path, value: str,
                        *, directory: bool = False) -> None:
    relative = PurePosixPath(value)
    if (not value or relative.is_absolute() or "\\" in value
            or any(part in ("", ".", "..") for part in value.split("/"))):
        raise AcceptanceFailure("P036 inventory contains a redirected source path")
    source = source_root.joinpath(*relative.parts)
    current = source_root
    for part in relative.parts:
        current = current / part
        info = current.lstat()
        if stat.S_ISLNK(info.st_mode):
            raise AcceptanceFailure("P036 artifact input contains a symlink")
    destination = destination_root.joinpath(*relative.parts)
    if directory:
        if not source.is_dir():
            raise AcceptanceFailure("P036 runtime payload root is not a directory")
        for at, dirs, files in os.walk(source, topdown=True, followlinks=False):
            dirs.sort()
            files.sort()
            relative_dir = Path(at).relative_to(source)
            target_dir = destination / relative_dir
            target_dir.mkdir(parents=True, exist_ok=True)
            for name in dirs:
                directory_path = Path(at) / name
                if not stat.S_ISDIR(directory_path.lstat().st_mode):
                    raise AcceptanceFailure("P036 runtime payload contains a directory symlink")
            for name in files:
                source_file = Path(at) / name
                metadata = source_file.lstat()
                if (not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1
                        or metadata.st_size > MAX_ARTIFACT_BYTES):
                    raise AcceptanceFailure("P036 runtime payload contains a redirected file")
                shutil.copyfile(source_file, target_dir / name, follow_symlinks=False)
        return
    info = source.lstat()
    if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
            or info.st_size > MAX_ARTIFACT_BYTES):
        raise AcceptanceFailure("P036 artifact input is not a single-link regular file")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination, follow_symlinks=False)


def verify_inventory(platform_name: str) -> tuple[dict, Path, tempfile.TemporaryDirectory]:
    inventory = ROOT / "acceptance/p036/production-artifacts.json"
    if not inventory.is_file():
        raise BlockedExternalInput("production-artifacts.json has not been supplied")
    install_build_dependencies(platform_name)
    validator = ROOT / "scripts/validate-p036-production-artifacts.py"
    with tempfile.TemporaryDirectory(prefix="synveil-p036-validator-") as temporary:
        environment = Path(temporary)
        run([sys.executable, "-m", "venv", str(environment)], timeout=120)
        python = environment / "bin/python"
        run([str(python), "-m", "pip", "install", "--quiet", "-r",
             str(ROOT / "scripts/requirements-p036.txt")], timeout=300)
        result = run(
            [str(python), str(validator), "--platform", platform_name, "--print-json"],
            timeout=600, env=os.environ.copy(), check=False,
        )
    if result.returncode == 2:
        raise BlockedExternalInput(result.stderr.strip() or "trusted production artifact inputs are missing")
    if result.returncode:
        raise AcceptanceFailure("P036 production artifact inventory was rejected: " + result.stderr.strip()[-500:])
    try:
        normalized = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise AcceptanceFailure("P036 artifact validator did not return a normalized inventory") from error
    if normalized.get("schema_version") != 1 or len(normalized.get("artifacts", [])) != 4:
        raise AcceptanceFailure("P036 artifact validator did not verify all four native components")
    staging = tempfile.TemporaryDirectory(prefix="synveil-p036-verified-")
    staging_root = Path(staging.name)
    artifact_root = staging_root / "artifacts"
    artifact_root.mkdir(mode=0o700)
    try:
        raw_inventory = inventory.read_bytes()
        document = json.loads(raw_inventory, object_pairs_hook=unique_json_object)
        copy_confined_input(ROOT, staging_root, "acceptance/p036/production-artifacts.json")
        copy_confined_input(ROOT, staging_root, "acceptance/p036/production-artifacts.json.sig")
        for entry in document["artifacts"]:
            if entry["platform"] != platform_name:
                continue
            artifact = entry["artifact"]
            if "payload_root" in artifact:
                copy_confined_input(ROOT / "acceptance/p036/artifacts", artifact_root,
                                    artifact["payload_root"], directory=True)
                copy_confined_input(ROOT / "acceptance/p036/artifacts", artifact_root,
                                    artifact["signature_path"])
                copy_confined_input(ROOT / "acceptance/p036/artifacts", artifact_root,
                                    entry["provenance"]["signature_path"])
                continue
            for value in (artifact["path"], artifact["signature_path"],
                          entry["provenance"]["path"], entry["provenance"]["signature_path"],
                          entry["license"]["path"], entry["license"]["notice_path"]):
                copy_confined_input(ROOT / "acceptance/p036/artifacts", artifact_root, value)
        staged_inventory = staging_root / "acceptance/p036/production-artifacts.json"
        staged_signature = Path(str(staged_inventory) + ".sig")
        result = run(
            [str(python), str(validator), "--platform", platform_name, "--print-json",
             "--inventory", str(staged_inventory), "--inventory-signature", str(staged_signature),
             "--artifact-root", str(artifact_root)],
            timeout=600, env=os.environ.copy(), check=False,
        )
        if result.returncode == 2:
            raise BlockedExternalInput(result.stderr.strip() or "trusted staged production inputs are missing")
        if result.returncode:
            raise AcceptanceFailure("staged P036 artifacts failed independent verification: " + result.stderr.strip()[-500:])
        normalized = json.loads(result.stdout)
        for item in normalized["artifacts"]:
            if item.get("payload_root"):
                item["payload_path"] = str(artifact_root / item["payload_root"])
    except Exception:
        staging.cleanup()
        raise
    return normalized, Path(os.environ["SYNVEIL_P036_TRUSTED_KEYRING"]), staging


def install_package(platform_name: str, package: Path) -> None:
    if platform_name == "ubuntu-24.04":
        root_run(["/usr/bin/apt-get", "install", "--yes", "--no-install-recommends", str(package)], timeout=900)
    else:
        dnf = shutil.which("dnf")
        if dnf is None:
            raise BlockedEnvironment("Fedora dnf package manager is unavailable")
        root_run([dnf, "install", "--assumeyes", str(package)], timeout=900)


def install_build_dependencies(platform_name: str) -> None:
    if platform_name == "ubuntu-24.04":
        apt = "/usr/bin/apt-get"
        root_run([apt, "-qq", "update"], timeout=900)
        root_run([
            apt, "-qq", "install", "--yes", "--no-install-recommends",
            "build-essential", "pkg-config", "libdbus-1-dev", "python3-venv",
        ], timeout=900)
        return
    dnf = shutil.which("dnf")
    if dnf is None:
        raise BlockedEnvironment("Fedora dnf package manager is unavailable")
    root_run([dnf, "-q", "install", "--assumeyes", "gcc", "gcc-c++", "make",
              "pkgconf-pkg-config", "dbus-devel", "python3-pip"], timeout=900)


def require_all_active(units: tuple[str, ...]) -> None:
    for unit in units:
        run(["/usr/bin/systemctl", "is-active", "--quiet", unit], timeout=30)


def reinstall_package(platform_name: str, package: Path) -> None:
    if platform_name == "ubuntu-24.04":
        root_run(["/usr/bin/apt-get", "install", "--yes", "--reinstall", "--no-install-recommends", str(package)], timeout=900)
    else:
        dnf = shutil.which("dnf")
        if dnf is None:
            raise BlockedEnvironment("Fedora dnf package manager is unavailable")
        root_run([dnf, "reinstall", "--assumeyes", str(package)], timeout=900)


def assemble_and_register(platform_name: str, entries: list[dict], keyring: Path) -> None:
    by_component = {item["component"]: item for item in entries}
    runtime_kinds = {"server": "server", "postgresql17": "postgresql", "edge": "caddy"}
    with tempfile.TemporaryDirectory(prefix="synveil-p036-inputs-") as temporary:
        directory = Path(temporary)
        directory.chmod(0o700)
        registration = {"schema_version": 1, "qualified_os": platform_name, "artifacts": {}}
        for component, kind in runtime_kinds.items():
            item = by_component[component]
            registration["artifacts"][kind] = {
                "manifest": item["artifact_path"],
                "signature": item["artifact_signature_path"],
                "trusted_keyring": str(keyring),
                "input": item["payload_path"],
            }
        registration_path = directory / "registration.json"
        registration_path.write_text(json.dumps(registration, sort_keys=True), encoding="utf-8")
        registration_path.chmod(0o600)
        bundle = directory / "bundle"
        run([
            sys.executable, str(ROOT / "deploy/server/assemble-host-inputs.py"),
            "--inputs", str(registration_path), "--output", str(bundle),
        ], timeout=1800)
        root_run([
            "/usr/bin/python3", "-I", "/usr/libexec/synveil-host/register-host-inputs.py",
            "--bundle", str(bundle), "--server-keyring", str(keyring),
            "--postgresql-keyring", str(keyring), "--caddy-keyring", str(keyring),
        ], timeout=1800)


def snapshot(*, preserved_only: bool = False) -> dict:
    argv = ["/usr/bin/python3", str(ROOT / "scripts/p036-snapshot-state.py")]
    if preserved_only:
        argv.append("--preserved-only")
    result = root_run(argv, timeout=300)
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise AcceptanceFailure("native state snapshot was not valid JSON") from error


def require_stable_state(before: dict, after: dict, label: str) -> None:
    keys = (
        "server_installation_id", "storage_id", "storage_root_identity",
        "network_integration_id", "postgres_system_identifier", "postgres_data_identity",
        "postgres_runtime_identity", "managed_ca_sha256",
        "administrator_id", "library_id", "credential_sha256", "server_objects_sha256",
        "postgres_private", "services_ready", "https_ready",
    )
    changed = [key for key in keys if before.get(key) != after.get(key)]
    if changed:
        raise AcceptanceFailure(f"{label} changed protected Host identity/state: {', '.join(changed)}")


def verify_systemd_units() -> None:
    units = [
        "/etc/systemd/system/synveil-server.target",
        "/etc/systemd/system/synveil-postgresql.service",
        "/etc/systemd/system/synveil-server-migrate.service",
        "/etc/systemd/system/synveil-api.service",
        "/etc/systemd/system/synveil-worker.service",
        "/etc/systemd/system/synveil-edge.service",
    ]
    if any(not Path(unit).is_file() for unit in units):
        raise AcceptanceFailure("installed Managed Host unit topology is incomplete")
    root_run(["/usr/bin/systemd-analyze", "verify", *units], timeout=120)


def main() -> int:
    if len(sys.argv) != 2:
        raise BlockedEnvironment("usage: p036_native_acceptance.py ubuntu-24.04|fedora-42")
    check_disposable_runner()
    check_authorization_session()
    platform_name = host_platform()
    if platform_name != sys.argv[1]:
        raise BlockedEnvironment("requested platform and detected platform differ")
    require_clean_host()
    normalized, keyring, artifact_stage = verify_inventory(platform_name)
    entries = normalized["artifacts"]
    desktop = next(item for item in entries if item["component"] == "desktop")
    package = Path(desktop["artifact_path"]).resolve(strict=True)
    install_package(platform_name, package)
    assemble_and_register(platform_name, entries, keyring)

    runner_flag = os.environ.copy()
    runner_flag["SYNVEIL_P036_NATIVE_ACCEPTANCE"] = "1"
    native_bin = [
        "cargo", "run", "--release", "--locked", "--features", "p036-acceptance",
        "-p", "synveil-server-host", "--bin", "synveil-p036-native-acceptance", "--", "fresh",
    ]
    result = run(native_bin, timeout=3600, env=runner_flag)
    print(result.stdout, end="")

    before = snapshot()
    root_run(["/usr/bin/systemctl", "restart", "synveil-server.target"], timeout=180)
    require_all_active(("synveil-postgresql.service", "synveil-api.service",
                        "synveil-worker.service", "synveil-edge.service"))
    verify_systemd_units()
    run(["cargo", "run", "--release", "--locked", "--features", "p036-acceptance", "-p", "synveil-server-host", "--bin", "synveil-p036-native-acceptance", "--", "verify-existing"], timeout=900, env=runner_flag)
    after_restart = snapshot()
    require_stable_state(before, after_restart, "service restart")

    reinstall_package(platform_name, package)
    root_run(["/usr/bin/systemctl", "daemon-reload"], timeout=60)
    root_run(["/usr/bin/systemctl", "restart", "synveil-server.target"], timeout=180)
    require_all_active(("synveil-postgresql.service", "synveil-api.service",
                        "synveil-worker.service", "synveil-edge.service"))
    run(["cargo", "run", "--release", "--locked", "--features", "p036-acceptance", "-p", "synveil-server-host", "--bin", "synveil-p036-native-acceptance", "--", "repair-existing"], timeout=1200, env=runner_flag)
    after_repair = snapshot()
    require_stable_state(after_restart, after_repair, "package reinstall and preserving repair")

    run(native_bin[:-1] + ["remove-software-preserving"], timeout=1200, env=runner_flag)
    after_removal = snapshot(preserved_only=True)
    preserved_keys = (
        "server_installation_id", "storage_id", "storage_root_identity",
        "network_integration_id", "postgres_system_identifier", "postgres_data_identity",
        "postgres_runtime_identity", "managed_ca_sha256", "administrator_id",
        "library_id", "credential_sha256", "server_objects_sha256", "postgres_private",
    )
    changed = [key for key in preserved_keys if after_repair.get(key) != after_removal.get(key)]
    if changed:
        raise AcceptanceFailure("software removal changed protected Host state: " + ", ".join(changed))
    run(native_bin[:-1] + ["repair-existing"], timeout=1200, env=runner_flag)
    after_software_repair = snapshot()
    require_stable_state(after_repair, after_software_repair,
                         "data-preserving software removal and repair")
    print(json.dumps({
        "evidence": "PASS_NATIVE",
        "platform": platform_name,
        "journey": "fresh_host_first_sync",
        "qml_surface": "NOT_EXECUTED; production coordinator and DesktopController APIs are exercised directly",
        "service_restart": "passed",
        "package_reinstall": "passed",
        "repair": "passed",
        "remove_software_preservation": "passed",
        "preserved_identity": True,
        "reboot": "NOT_EXECUTED; covered by the disposable VM interruption job",
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BlockedExternalInput as error:
        print(f"BLOCKED_BY_EXTERNAL_INPUT: {error}", file=sys.stderr)
        raise SystemExit(2)
    except BlockedEnvironment as error:
        print(f"BLOCKED_ENVIRONMENT: {error}", file=sys.stderr)
        raise SystemExit(2)
    except AcceptanceFailure as error:
        print(f"FAILED: {error}", file=sys.stderr)
        raise SystemExit(1)
