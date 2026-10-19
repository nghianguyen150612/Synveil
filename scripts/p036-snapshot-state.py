#!/usr/bin/python3
"""Emit bounded, secret-free authoritative P036 preservation evidence."""
from __future__ import annotations

import hashlib
import json
import os
import platform
from pathlib import Path
import pwd
import re
import stat
import subprocess
import sys
from urllib.parse import urljoin, urlsplit

CONFIG = Path("/etc/synveil/server-config.json")
JOURNAL = Path("/etc/synveil/host-setup.json")
PG_DATA = Path("/var/lib/synveil/postgresql/17/data")
RUNTIME_CATALOG = Path("/etc/synveil/host-runtime-catalog.json")
PG_CONFIG = Path("/etc/synveil/postgresql/postgresql.conf")
MAX_TREE_FILES = 100_000
MAX_TREE_BYTES = 4 * 1024 * 1024 * 1024


def regular_bytes(path: Path, limit: int) -> bytes:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > limit:
        raise ValueError("protected state file has unsafe type or bounds")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        with os.fdopen(fd, "rb", closefd=False) as stream:
            data = stream.read(limit + 1)
    finally:
        os.close(fd)
    if len(data) > limit:
        raise ValueError("protected state file exceeded bounds")
    return data


def owned_json(path: Path, limit: int, owner_uid: int = 0, expected_mode: int = 0o600,
               expected_gid: int | None = None) -> dict:
    info = path.lstat()
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != owner_uid or info.st_nlink != 1
            or stat.S_IMODE(info.st_mode) != expected_mode
            or (expected_gid is not None and info.st_gid != expected_gid)):
        raise ValueError("protected Host state ownership or mode is unsafe")
    try:
        value = json.loads(regular_bytes(path, limit), object_pairs_hook=unique_object)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise ValueError("Host configuration is unreadable") from error
    if not isinstance(value, dict):
        raise ValueError("Host configuration shape is invalid")
    return value


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate protected JSON key")
        result[key] = value
    return result


def find_values(value, key: str) -> list:
    found = []
    if isinstance(value, dict):
        for name, child in value.items():
            if name == key:
                found.append(child)
            found.extend(find_values(child, key))
    elif isinstance(value, list):
        for child in value:
            found.extend(find_values(child, key))
    return found


def find_network_integration(value: object) -> dict:
    if isinstance(value, dict):
        if {"network_integration_id", "mode", "canonical_origin", "listener"} <= value.keys():
            return value
        for child in value.values():
            try:
                return find_network_integration(child)
            except ValueError:
                continue
    elif isinstance(value, list):
        for child in value:
            try:
                return find_network_integration(child)
            except ValueError:
                continue
    raise ValueError("network integration identity is unavailable")


def run(argv: list[str], timeout: int = 20) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, shell=False, timeout=timeout, check=False,
        env={"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LC_ALL": "C", "LANG": "C"},
    )
    if result.returncode != 0 or len(result.stdout) > 1_000_000:
        raise ValueError("authoritative native Host query failed")
    return result


def file_tree_snapshot(root: Path) -> tuple[str, int, int]:
    info = root.lstat()
    if not stat.S_ISDIR(info.st_mode):
        raise ValueError("server data root is not a directory")
    if root == Path("/var/lib/synveil/storage") and info.st_uid != pwd.getpwnam("synveil").pw_uid:
        raise ValueError("server object storage owner differs from synveil")
    if root == PG_DATA and info.st_uid != pwd.getpwnam("synveil-postgres").pw_uid:
        raise ValueError("PostgreSQL data directory owner differs from synveil-postgres")
    digest = hashlib.sha256()
    files_seen = 0
    total_bytes = 0
    for directory, dirs, files in os.walk(root, topdown=True, followlinks=False):
        dirs.sort()
        files.sort()
        for name in dirs:
            child = Path(directory) / name
            metadata = child.lstat()
            if not stat.S_ISDIR(metadata.st_mode):
                raise ValueError("server data contains a redirected directory")
        for name in files:
            child = Path(directory) / name
            metadata = child.lstat()
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
                raise ValueError("server data contains a redirected or non-regular file")
            files_seen += 1
            total_bytes += metadata.st_size
            if files_seen > MAX_TREE_FILES or total_bytes > MAX_TREE_BYTES:
                raise ValueError("server data tree exceeds the bounded P036 snapshot size")
            relative = child.relative_to(root).as_posix().encode()
            digest.update(len(relative).to_bytes(4, "big"))
            digest.update(relative)
            digest.update(metadata.st_size.to_bytes(8, "big"))
            file_digest = hashlib.sha256()
            with child.open("rb") as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    file_digest.update(block)
            digest.update(file_digest.digest())
    return digest.hexdigest(), files_seen, total_bytes


def systemd_properties(unit: str) -> dict[str, str]:
    values = run(["/usr/bin/systemctl", "show", unit,
                  "--property=ActiveState,SubState,Result,ExecMainStatus,User,Group,MainPID"]).stdout
    return dict(line.split("=", 1) for line in values.splitlines() if "=" in line)


def listen_addresses() -> list[str]:
    return [line.split()[3] for line in run(["/usr/bin/ss", "-H", "-ltn"]).stdout.splitlines() if len(line.split()) >= 4]


def require_private_listener(addresses: list[str], port: int) -> None:
    bound = [item for item in addresses if item.rsplit(":", 1)[-1] == str(port)]
    if bound != [f"127.0.0.1:{port}"]:
        raise ValueError("PostgreSQL listener is absent or differs from its persisted loopback endpoint")


def require_local_api(addresses: list[str]) -> None:
    bound = [item for item in addresses if item.rsplit(":", 1)[-1] == "3000"]
    if not bound or any(item.startswith(("0.0.0.0:", "*:", "[::]:")) for item in bound):
        raise ValueError("Synveil API listener is absent or publicly bound")
    if any(not item.startswith(("127.0.0.1:", "[::1]:")) for item in bound):
        raise ValueError("Synveil API listener is bound outside loopback")


def main() -> int:
    preserved_only = sys.argv[1:] == ["--preserved-only"]
    if len(sys.argv) > 2 or (len(sys.argv) == 2 and not preserved_only):
        raise ValueError("snapshot mode must be omitted or --preserved-only")
    if os.geteuid() != 0:
        raise ValueError("P036 state snapshot requires root through the native runner's bounded sudo policy")
    release = {}
    for line in Path("/etc/os-release").read_text(encoding="utf-8").splitlines():
        key, separator, value = line.partition("=")
        if separator:
            release[key] = value.strip().strip('"')
    host_platform = {("ubuntu", "24.04"): "ubuntu-24.04", ("fedora", "42"): "fedora-42"}.get(
        (release.get("ID"), release.get("VERSION_ID"))
    )
    if host_platform is None or platform.machine().lower() not in {"x86_64", "amd64"}:
        raise ValueError("P036 snapshot requires supported Ubuntu/Fedora x86_64")
    if not Path("/run/systemd/system").is_dir():
        raise ValueError("P036 snapshot requires the native systemd system manager")
    synveil = pwd.getpwnam("synveil")
    config = owned_json(CONFIG, 128 * 1024, expected_mode=0o640, expected_gid=synveil.pw_gid)
    journal = owned_json(JOURNAL, 128 * 1024)
    server_id = config.get("server_installation_id")
    if not isinstance(server_id, str) or server_id != journal.get("server_id"):
        raise ValueError("server installation identity is inconsistent")
    storage = config.get("storage")
    storage_ids = find_values(storage, "storage_id")
    roots = find_values(storage, "root")
    if len(storage_ids) != 1 or len(roots) != 1 or not isinstance(roots[0], str):
        raise ValueError("configured storage identity is unavailable")
    if (not isinstance(storage, dict) or storage.get("state") != "configured_local"
            or config.get("deployment_profile") != "personal_home_managed"):
        raise ValueError("server storage is not authoritatively configured")
    storage_root = Path(roots[0])
    if not storage_root.is_absolute() or any(part in (".", "..") for part in storage_root.parts):
        raise ValueError("configured storage path is unsafe")
    storage_identity = find_values(storage, "root_identity")
    storage_info = storage_root.lstat()
    synveil_storage = pwd.getpwnam("synveil")
    if (len(storage_identity) != 1 or not stat.S_ISDIR(storage_info.st_mode)
            or storage_info.st_uid != synveil_storage.pw_uid
            or storage_info.st_gid != synveil_storage.pw_gid
            or storage_identity[0] != {"kind": "directory", "device": storage_info.st_dev,
                                       "inode": storage_info.st_ino}):
        raise ValueError("configured storage root identity is unavailable")

    database = config.get("database")
    endpoint = database.get("endpoint") if isinstance(database, dict) else None
    if (not isinstance(database, dict) or database.get("postgres_major") != 17
            or database.get("mode") != "managed_private"
            or database.get("ownership") != "synveil_managed"
            or database.get("credential_state") != "materialized"
            or not isinstance(endpoint, dict) or endpoint.get("host") != "127.0.0.1"):
        raise ValueError("configured database is not PostgreSQL 17")
    port = journal.get("postgres_port")
    if type(port) is not int or not 1 <= port <= 65535 or endpoint.get("port") != port:
        raise ValueError("persisted managed PostgreSQL port is unavailable")
    pg_config_info = PG_CONFIG.lstat()
    if (not stat.S_ISREG(pg_config_info.st_mode) or pg_config_info.st_uid != 0
            or pg_config_info.st_gid != 0 or pg_config_info.st_nlink != 1
            or stat.S_IMODE(pg_config_info.st_mode) != 0o644):
        raise ValueError("managed PostgreSQL configuration ownership or mode is unsafe")
    config_text = regular_bytes(PG_CONFIG, 16 * 1024).decode("utf-8", "strict")
    configured_ports = re.findall(r"(?m)^port\s*=\s*([0-9]+)\s*$", config_text)
    if configured_ports != [str(port)]:
        raise ValueError("PostgreSQL configured port does not match persisted Host identity")

    pg_data_info = PG_DATA.lstat()
    pg_account = pwd.getpwnam("synveil-postgres")
    pg_uid = pg_account.pw_uid
    if (not stat.S_ISDIR(pg_data_info.st_mode) or pg_data_info.st_uid != pg_uid
            or pg_data_info.st_gid != pg_account.pw_gid
            or stat.S_IMODE(pg_data_info.st_mode) != 0o700):
        raise ValueError("PostgreSQL data directory is not private to synveil-postgres")
    catalog = owned_json(RUNTIME_CATALOG, 16 * 1024, expected_mode=0o644, expected_gid=0)
    if (catalog.get("schema_version") != 1 or catalog.get("qualified_os") != host_platform
            or not isinstance(catalog.get("artifacts"), dict)
            or set(catalog["artifacts"]) != {"server", "postgresql", "caddy"}):
        raise ValueError("verified Host runtime catalog is invalid")
    runtime_id = catalog["artifacts"].get("postgresql")
    if not isinstance(runtime_id, str) or not re.fullmatch(r"[0-9a-f]{64}", runtime_id):
        raise ValueError("PostgreSQL runtime identity is unavailable from the verified catalog")
    runtime = ((Path("/var/cache/synveil/host-runtime") if preserved_only
                else Path("/opt/synveil/postgresql/17")) / runtime_id)
    if preserved_only:
        runtime_roots = {
            "server": Path("/opt/synveil/server"),
            "postgresql": Path("/opt/synveil/postgresql/17"),
            "caddy": Path("/opt/synveil/caddy"),
        }
        if any((runtime_roots[kind] / digest).exists()
               for kind, digest in catalog["artifacts"].items() if kind in runtime_roots):
            raise ValueError("a hosted runtime was not removed with server software")
    runtime_info = runtime.lstat()
    if (not stat.S_ISDIR(runtime_info.st_mode) or runtime_info.st_uid != 0
            or runtime_info.st_gid != 0 or stat.S_IMODE(runtime_info.st_mode) != 0o755):
        raise ValueError("managed PostgreSQL runtime escapes its approved versioned root")
    manifest_path = runtime / "manifest.json"
    manifest = owned_json(manifest_path, 2 * 1024 * 1024, expected_mode=0o644, expected_gid=0)
    if (hashlib.sha256(regular_bytes(manifest_path, 2 * 1024 * 1024)).hexdigest() != runtime_id
            or manifest.get("kind") != "postgresql"
            or not isinstance(manifest.get("version"), str)
            or manifest["version"].split(".", 1)[0] != "17"
            or manifest.get("qualified_os") != catalog["qualified_os"]
            or manifest.get("architecture") != "x86_64"):
        raise ValueError("PostgreSQL runtime manifest identity does not match its catalog")
    version_result = run([str(runtime / "bin/postgres"), "--version"])
    match = re.fullmatch(r"postgres \(PostgreSQL\) (17\.[A-Za-z0-9.+_-]+)\n?", version_result.stdout)
    if not match:
        raise ValueError("verified PostgreSQL executable does not report major version 17")
    control = run(["/usr/sbin/runuser", "-u", "synveil-postgres", "--",
                   str(runtime / "bin/pg_controldata"), str(PG_DATA)]).stdout
    system_ids = re.findall(r"(?m)^Database system identifier:\s*([0-9]+)\s*$", control)
    if len(system_ids) != 1 or journal.get("postgres_system_id") != system_ids[0]:
        raise ValueError("PostgreSQL system identity differs from the Host journal")
    credentials = Path("/etc/synveil/credentials")
    credential_hashes = {}
    for name in ("database-password", "database-url", "rebaseline-token-key",
                 "network-ca-key", "network-tls-key"):
        path = credentials / name
        info = path.lstat()
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_gid != 0
                or info.st_nlink != 1 or stat.S_IMODE(info.st_mode) != 0o600):
            raise ValueError("protected Host credential ownership or mode is unsafe")
        credential_hashes[name] = hashlib.sha256(regular_bytes(path, 8192)).hexdigest()

    services = {
        "synveil-postgresql.service": ("synveil-postgres", "synveil-postgres", "active"),
        "synveil-api.service": ("synveil", "synveil", "active"),
        "synveil-worker.service": ("synveil", "synveil", "active"),
        "synveil-edge.service": ("synveil-edge", "synveil-edge", "active"),
    }
    for unit, (user, group, active_state) in services.items():
        if preserved_only:
            unit_path = Path("/etc/systemd/system") / unit
            if unit_path.exists() or unit_path.is_symlink():
                raise ValueError("hosted service unit remains after preserving software removal")
            continue
        values = systemd_properties(unit)
        if values.get("User") != user or values.get("Group") != group or values.get("ActiveState") != active_state:
            raise ValueError("Managed Host service owner or active state is incorrect")
        if not values.get("MainPID", "0").isdigit() or values.get("MainPID") == "0":
            raise ValueError("Managed Host service readiness PID is unavailable")
    if preserved_only:
        if (Path("/etc/systemd/system/synveil-server-migrate.service").exists()
                or Path("/etc/systemd/system/synveil-server.target").exists()
                or Path("/etc/synveil/network/Caddyfile").exists()):
            raise ValueError("owned unit or edge config remains after preserving software removal")
        if Path("/etc/systemd/system/multi-user.target.wants/synveil-server.target").exists():
            raise ValueError("Hosted Host target remains enabled after software removal")
    else:
        migration = systemd_properties("synveil-server-migrate.service")
        if migration.get("Result") != "success" or migration.get("ExecMainStatus") != "0":
            raise ValueError("migration oneshot has not completed successfully")
        if systemd_properties("synveil-server.target").get("ActiveState") != "active":
            raise ValueError("Managed Host systemd target is not active")
        enabled = run(["/usr/bin/systemctl", "is-enabled", "synveil-server.target"]).stdout.strip()
        if enabled != "enabled":
            raise ValueError("Managed Host target is not boot-enabled")

    addresses = listen_addresses() if not preserved_only else []
    if not preserved_only:
        require_private_listener(addresses, port)
        require_local_api(addresses)
    network = find_network_integration(config.get("network"))
    origin = network["canonical_origin"]
    parsed = urlsplit(origin)
    if parsed.scheme != "https" or parsed.hostname not in {"localhost", "127.0.0.1", "::1"}:
        raise ValueError("Personal/Home Host canonical origin is not local HTTPS")
    if (network["mode"] != "local_only"
            or parsed.username is not None or parsed.password is not None
            or parsed.path != "/" or parsed.query or parsed.fragment
            or parsed.port not in (None, 443)):
        raise ValueError("native acceptance expected the Local Only access mode")
    if (network.get("backend_endpoint") != "http://127.0.0.1:3000"
            or network.get("trust", {}).get("mode") != "managed_private_ca"):
        raise ValueError("Personal/Home Host must keep its API loopback and managed TLS trust")
    listener = network["listener"]
    listener_address = listener.get("address") if isinstance(listener, dict) else None
    listener_port = listener.get("port") if isinstance(listener, dict) else None
    if (listener_address != "127.0.0.1" or listener_port != 443
            or parsed.hostname != "localhost"):
        raise ValueError("Local Only HTTPS listener is not bound to a loopback interface")
    if not preserved_only:
        edge_bound = [item for item in addresses if item.rsplit(":", 1)[-1] == str(listener_port)]
        if edge_bound != ["127.0.0.1:443"]:
            raise ValueError("Local Only HTTPS listener is not bound exclusively to its reviewed address")
    ca = Path("/etc/synveil/network/ca-certificate.pem")
    ca_info = ca.lstat()
    if (not stat.S_ISREG(ca_info.st_mode) or ca_info.st_uid != 0 or ca_info.st_gid != 0
            or ca_info.st_nlink != 1 or stat.S_IMODE(ca_info.st_mode) != 0o644):
        raise ValueError("managed TLS trust certificate is unavailable or unsafe")
    if not preserved_only:
        health_url = urljoin(origin.rstrip("/") + "/", "health/ready")
        run(["/usr/bin/curl", "--fail", "--silent", "--show-error", "--max-time", "15",
             "--cacert", str(ca), "--output", "/dev/null", health_url])
        worker_health = owned_json(Path("/run/synveil-worker/health.json"), 16 * 1024,
                                   synveil.pw_uid, expected_gid=synveil.pw_gid)
        if (worker_health.get("server_id") != server_id
                or worker_health.get("storage_id") != storage_ids[0]
                or worker_health.get("database_ready") is not True
                or worker_health.get("storage_ready") is not True):
            raise ValueError("worker readiness evidence does not match the managed Host")

    admin_ids = find_values(journal, "verified_admin")
    library_ids = find_values(journal, "library_id")
    if len(admin_ids) != 1 or not isinstance(admin_ids[0], str) or not admin_ids[0]:
        raise ValueError("first administrator identity is not recorded")
    if len(library_ids) != 1 or not isinstance(library_ids[0], str) or not library_ids[0]:
        raise ValueError("initial Library identity is not recorded")
    storage_digest, storage_files, storage_bytes = file_tree_snapshot(storage_root)
    if not preserved_only and (storage_files == 0 or storage_bytes == 0):
        raise ValueError("initial first-sync did not leave server object data")
    ca_digest = hashlib.sha256(regular_bytes(ca, 1024 * 1024)).hexdigest()
    if network["trust"].get("ca_certificate_sha256") != ca_digest:
        raise ValueError("managed TLS certificate trust identity differs from configuration")
    result = {
        "evidence": "PASS_NATIVE",
        "server_installation_id": server_id,
        "storage_id": storage_ids[0],
        "storage_root_identity": storage_identity[0],
        "network_integration_id": network["network_integration_id"],
        "postgres_major": 17,
        "postgres_version": match.group(1),
        "postgres_runtime_identity": runtime_id,
        "postgres_data_identity": {"device": pg_data_info.st_dev, "inode": pg_data_info.st_ino},
        "postgres_system_identifier": system_ids[0],
        "administrator_id": admin_ids[0],
        "library_id": library_ids[0],
        "credential_sha256": credential_hashes,
        "server_objects_sha256": storage_digest,
        "server_object_files": storage_files,
        "server_object_bytes": storage_bytes,
        "managed_ca_sha256": ca_digest,
        "postgres_private": True,
        "api_loopback": True,
        "services_ready": not preserved_only,
        "https_ready": not preserved_only,
    }
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, IndexError, subprocess.SubprocessError) as error:
        print(f"BLOCKED_OR_FAILED: {type(error).__name__}", file=sys.stderr)
        raise SystemExit(1)
