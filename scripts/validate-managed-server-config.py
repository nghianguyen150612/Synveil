#!/usr/bin/env python3
"""Network-free static guard for the Prompt031 configuration boundary."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FAILURES: list[str] = []


def read(relative: str) -> str:
    path = ROOT / relative
    if not path.is_file():
        FAILURES.append(f"missing required file: {relative}")
        return ""
    return path.read_text(encoding="utf-8")


def require(label: str, condition: bool) -> None:
    if not condition:
        FAILURES.append(label)


model = read("crates/server-config/src/model.rs")
secret = read("crates/server-config/src/secret.rs")
store = read("crates/server-config/src/store.rs")
db_loader = read("crates/api/src/runtime_database_credential.rs")
key_loader = read("crates/api/src/runtime_rebaseline_credential.rs")
api_source = read("crates/api/src/bin/synveil-api.rs")
worker_source = read("crates/api/src/bin/synveil-worker.rs")
selector = read("crates/api/src/runtime_server_configuration.rs")
roadmap = read("docs/v0.2/ROADMAP.md")
adr = read("docs/adr/ADR-061-v0.2-managed-server-configuration.md")
manifest = read("docs/v0.2/PROMPT031_MANIFEST.md")

require("schema version 1 is explicit", "SERVER_CONFIG_SCHEMA_VERSION: u32 = 1" in model)
require("managed schema objects deny unknown fields", "deny_unknown_fields" in model)
require("managed and Advanced profiles are explicit", "PersonalHomeManaged" in model and "AdvancedExternal" in model)
require("PostgreSQL 17 is enforced by validation", "postgres_major != 17" in model and "postgres_major: 17" in model)
require("installation identity is canonical UUIDv7", "get_version_num() != 7" in model and "Uuid::now_v7()" in store)
require("fixed credential references exist", all(value in secret for value in ('"database-url"', '"database-password"', '"rebaseline-token-key"')))
require("managed config has references rather than secret values", "credential_ref: CredentialId" in model and "rebaseline_token_key_ref: CredentialId" in model)
require("storage pending state and typed update are present", "NotConfigured" in model and "update_storage" in store)
require("network pending state and local-only update are present", "NetworkConfiguration::NotConfigured" in model and "update_network_local_private" in store)
require("canonical config is bounded", "MAX_SERVER_CONFIG_BYTES: usize = 32 * 1024" in model and "MAX_SERVER_CONFIG_BYTES" in store)
require("Linux config path and permission policy are present", '"/etc/synveil"' in store and "CONFIG_DIRECTORY_MODE: u32 = 0o750" in store and "CONFIG_FILE_MODE: u32 = 0o640" in store)
require("Linux credential source modes are present", "CREDENTIAL_DIRECTORY_MODE: u32 = 0o700" in store and "SECRET_FILE_MODE: u32 = 0o600" in store)
require("config writes use atomic replacement and sync", "fs::rename(&temp_path" in store and "sync_directory(&self.layout.root)" in store and "verify_regular_file" in store)
require("config updates compare the reviewed fingerprint", "ConcurrentModification" in store and "generation" in store)
require("secret creation is exclusive and durable", "create_new(true)" in store and "file.sync_all()" in store and "SecretAlreadyExists" in store)
require("secret wrappers redact both formatting traits", "SecretMaterial([REDACTED])" in secret and "impl fmt::Display for SecretMaterial" in secret)
require("managed database password is generated before endpoint materialization", "generate_hex_secret" in store and "managed_database_password" in store and "materialize_managed_database_url" in store)
require("managed database credentials never become config values", "database_url" not in model and "database_password" not in model)

require("database runtime uses bounded protected file loading", "MAX_CREDENTIAL_FILE_SIZE" in db_loader and "O_NOFOLLOW" in db_loader and "same_file_identity" in db_loader)
require("database runtime rejects file plus environment", "AmbiguousConfiguration" in db_loader and "both credential file and DATABASE_URL are set" in db_loader)
require("rebaseline runtime loads the protected credential ID", "rebaseline-token-key" in key_loader and "O_NOFOLLOW" in key_loader)
require("rebaseline runtime rejects file plus environment", "AmbiguousConfiguration" in key_loader and "SYNVEIL_REBASELINE_TOKEN_KEY" in key_loader)
require("API and worker share the protected database runtime source", api_source.count("database_config_from_runtime()") >= 1 and worker_source.count("database_config_from_runtime()") >= 1)
require("API uses the protected rebaseline source", "rebaseline_key_from_runtime()" in api_source)
require("managed config selection is explicit and does not scan CWD", "SYNVEIL_SERVER_CONFIG_FILE" in selector and "LegacyOperator" in selector and "current-directory" in selector)
require("Linux managed path overrides retain production ownership checks", "at_managed_root" in selector and '#[cfg(target_os = "linux")]' in selector)
require("Linux-only default keeps non-Linux operator mode portable", '#[cfg(target_os = "linux")]' in selector and 'return Ok(RuntimeServerConfiguration::LegacyOperator)' in selector)
require("mixed managed and legacy sources fail closed", "AmbiguousAuthority" in selector and "has_legacy_configuration_inputs" in selector)
require("runtime credential tests serialize shared environment changes", "runtime_test_support::environment_lock" in db_loader and "runtime_test_support::environment_lock" in key_loader and "runtime_test_support::environment_lock" in selector)

for path in sorted((ROOT / "deploy/config").glob("*.env*")):
    content = path.read_text(encoding="utf-8")
    for number, line in enumerate(content.splitlines(), start=1):
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if re.match(r"(?:export\s+)?DATABASE_URL\s*=?", stripped):
            FAILURES.append(f"secret DATABASE_URL assignment in {path.relative_to(ROOT)}:{number}")
        if re.match(r"(?:export\s+)?SYNVEIL_REBASELINE_TOKEN_KEY\s*=?", stripped):
            FAILURES.append(f"secret rebaseline-key assignment in {path.relative_to(ROOT)}:{number}")

deploy_files = [
    *((ROOT / "deploy/systemd").glob("synveil-api*.service")),
    *((ROOT / "deploy/systemd").glob("synveil-worker*.service")),
]
require("P031 does not add API or worker service units", not deploy_files)
require("P031 does not add PostgreSQL provisioning commands", not re.search(r"\b(initdb|pg_ctl|createdb|createuser|psql)\b", store))
require("managed bind remains loopback-only", "is_loopback()" in model and "127.0.0.1:3000" in api_source)
require("P032-P035 remain explicit downstream boundaries", all(term in adr for term in ("P032", "P033", "P034", "P035")))
require("roadmap does not claim final Host readiness", "P032–P036 still own" in roadmap and "SYNVEIL_V0_2_GUIDED_SELF_HOSTING_READY" not in manifest)
require("P028 remains pending", "P028" in manifest and "pending" in manifest.lower())
require("P031 completion marker is recorded", "SYNVEIL_MANAGED_SERVER_CONFIGURATION_READY" in manifest)

if FAILURES:
    for failure in FAILURES:
        print(f"FAIL: {failure}", file=sys.stderr)
    raise SystemExit(1)

print("PASS: managed server configuration static invariants")
