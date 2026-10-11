#!/usr/bin/env python3
"""Network-free Prompt035 architecture and secret-hygiene checks."""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
errors = []

def read(path: str) -> str:
    target = ROOT / path
    if not target.is_file():
        errors.append(f"missing {path}")
        return ""
    return target.read_text(encoding="utf-8")

def require(path: str, *needles: str) -> None:
    value = read(path)
    for needle in needles:
        if needle not in value:
            errors.append(f"{path}: missing {needle}")

service = read("crates/auth/src/service.rs")
metadata = read("crates/metadata/src/auth.rs")
network = read("crates/server-network/src/lib.rs")
docs = read("docs/v0.2/SERVER_FIRST_ADMIN_BOOTSTRAP.md")
openapi = read("api/openapi.yaml")
config_model = read("crates/server-config/src/model.rs")
workflow = read(".github/workflows/server-first-admin-bootstrap.yml")

require("crates/auth/src/service.rs", "AdminBootstrapStatus", "inspect_admin_bootstrap", "verify_first_admin_login", "StoredPasswordHash::hash", ".create_first_admin")
require("crates/metadata/src/auth.rs", "FOR UPDATE", "insert_user", "insert_credential", "insert_initial_library", "SET state = 'CLOSED'")
require("crates/server-network/src/lib.rs", "render_managed_edge_config", "respond @first_admin 404", "reverse_proxy http://{}", "OperatorOwnedEdge")
require("api/openapi.yaml", "/api/v1/bootstrap/admin:", "loopback endpoint", "does not issue a browser session")
require("docs/v0.2/SERVER_FIRST_ADMIN_BOOTSTRAP.md", "machine-local", "explicitly perform normal authentication", "response is lost", "never reopen", "fails closed", "P036")
require("docs/adr/ADR-065-v0.2-managed-first-admin-bootstrap.md", "Status: **Accepted**", "sole authority", "exactly one", "No setup-secret")

if config_model.count("admin_created") or config_model.count("AdminBootstrapStatus"):
    errors.append("SERVER_CONFIG contains a duplicate bootstrap authority")
if metadata.count("FOR UPDATE") != 1:
    errors.append("expected the existing singleton bootstrap lock")
if "PlaintextPassword" not in service or "Argon2" not in read("crates/auth/src/passwords.rs"):
    errors.append("canonical Argon2id password boundary not retained")
if workflow.count("postgres_authentication_bootstrap_is_atomic_and_race_safe") != 2:
    errors.append("expected both live bootstrap scenarios to retain the exact integration test")
if "CREATE DATABASE synveil_bootstrap_repeat" not in workflow or "5432/synveil_bootstrap_repeat" not in workflow:
    errors.append("response-loss/restart bootstrap scenario must use a fresh PostgreSQL database")
prepare_repeat = workflow.find("Prepare a fresh database for response-loss and restart behavior")
repeat_test = workflow.find("Response-loss and restart behavior")
if prepare_repeat < 0 or repeat_test < 0 or prepare_repeat > repeat_test:
    errors.append("fresh repeat database must be created before the second bootstrap integration test")
if network.find("respond @first_admin 404") > network.find("reverse_proxy http://{}"):
    errors.append("managed deny must precede proxying")
for forbidden in ("bootstrap_" + "token", "?password=", "admin_created=true"):
    if forbidden in service + network + docs:
        errors.append(f"forbidden second-secret/authority pattern: {forbidden}")
premature = "SYNVEIL_V0_2_GUIDED_" + "SELF_HOSTING_READY"
for path in ("docs/v0.2/SERVER_FIRST_ADMIN_BOOTSTRAP.md", "docs/v0.2/PROMPT035_MANIFEST.md"):
    if premature in read(path):
        errors.append(f"{path}: premature P036 marker")

if errors:
    print("server first-admin validation failed:\n- " + "\n- ".join(errors), file=sys.stderr)
    sys.exit(1)
print("server first-admin bootstrap validation passed")
