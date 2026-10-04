#!/usr/bin/env python3
"""Network-free Prompt033 ownership and service-topology validation."""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
errors = []
def read(path):
    target = ROOT / path
    if not target.is_file(): errors.append(f"missing {path}"); return ""
    return target.read_text(encoding="utf-8")
def require(path, *needles):
    value = read(path)
    for needle in needles:
        if needle not in value: errors.append(f"{path}: missing {needle}")

desktop = read("deploy/install/MANIFEST")
server = read("deploy/server/MANIFEST")
for forbidden in ("synveil-api", "synveil-worker", "synveil-server-migrate", "/opt/synveil/postgresql"):
    if forbidden in desktop: errors.append(f"desktop manifest owns {forbidden}")
require("deploy/server/MANIFEST", "SERVER_API", "SERVER_WORKER", "SERVER_MIGRATE", "DATA, CONFIG, SECRET")
require("deploy/sysusers.d/synveil-postgres.conf", "u synveil-postgres", "/usr/sbin/nologin")
require("deploy/tmpfiles.d/synveil-postgresql.conf", "/var/lib/synveil/postgresql/17", "0700")
require("deploy/server/postgresql.conf", "listen_addresses = '127.0.0.1'", "password_encryption = 'scram-sha-256'")
for unit in ("synveil-api.service", "synveil-worker.service", "synveil-server-migrate.service"):
    path=f"deploy/systemd/{unit}"; value=read(path)
    require(path, "User=synveil", "Group=synveil", "LoadCredential=database-url:", "InaccessiblePaths=/etc/synveil/credentials")
    if "Environment=DATABASE_URL=" in value or "EnvironmentFile=" in value: errors.append(f"{path}: plaintext-capable secret delivery")
require("deploy/systemd/synveil-api.service", "LoadCredential=rebaseline-token-key:", "After=synveil-server-migrate.service")
require("deploy/systemd/synveil-worker.service", "SYNVEIL_GC_WORKER_ENABLED=1", "After=synveil-server-migrate.service synveil-api.service")
require("deploy/systemd/synveil-postgresql.service", "User=synveil-postgres", "RestrictAddressFamilies=AF_UNIX AF_INET", "InaccessiblePaths=/etc/synveil/credentials")
require("deploy/systemd/synveil-server.target", "WantedBy=multi-user.target")
require("deploy/server/systemd-managed/synveil-server.target.conf", "Wants=synveil-postgresql.service")
require("deploy/server/systemd-managed/synveil-server-migrate.service.conf", "Requires=synveil-postgresql.service")
require("crates/api/src/bin/synveil-server-migrate.rs", "MigrationRunner::new().run")
require("crates/server-service/src/lib.rs", "classify_cluster", "postgres_system_identifier", "StalePlan", "ExternalDatabaseOwnedByOperator")
if "chown -R" in "\n".join(read(p) for p in ["crates/server-service/src/lib.rs", "deploy/server/MANIFEST"]): errors.append("recursive chown is forbidden")
doc=read("docs/v0.2/SERVER_SERVICE_INSTALLATION.md")
for phrase in ("data-preserving", "AdvancedExternal", "P034", "P035", "INFRASTRUCTURE_READY"):
    if phrase not in doc: errors.append(f"service contract missing {phrase}")
if "SYNVEIL_V0_2_GUIDED_SELF_HOSTING_READY" in doc: errors.append("premature guided-hosting marker")
if errors:
    print("server-service validation failed:\n- " + "\n- ".join(errors), file=sys.stderr); sys.exit(1)
print("server-service installation validation passed")
