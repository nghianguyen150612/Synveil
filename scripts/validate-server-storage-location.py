#!/usr/bin/env python3
"""Network-free static guard for the Prompt032 server-storage boundary."""

from __future__ import annotations

import json
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


config_model = read("crates/server-config/src/model.rs")
config_store = read("crates/server-config/src/store.rs")
storage_crate = read("crates/server-storage/src/lib.rs")
view_model = read("crates/server-storage/src/model.rs")
wizard = read("crates/server-storage/src/wizard.rs")
path_source = read("crates/server-storage/src/path.rs")
identity = read("crates/server-storage/src/identity.rs")
storage_adapter = read("crates/storage/src/local.rs")
api = read("crates/api/src/bin/synveil-api.rs")
worker = read("crates/api/src/bin/synveil-worker.rs")
runtime_config = read("crates/api/src/runtime_server_configuration.rs")
contract = read("docs/v0.2/SERVER_STORAGE_LOCATION.md")
adr = read("docs/adr/ADR-062-v0.2-managed-server-storage-location.md")
roadmap = read("docs/v0.2/ROADMAP.md")
manifest = read("docs/v0.2/PROMPT032_MANIFEST.md")
validator_integration = read("scripts/validate-docs.sh")

def production_part(source: str) -> str:
    return re.split(r"(?m)^#\[cfg(?:\(test\)|\(all\(test)", source, maxsplit=1)[0]


production_wizard = production_part(wizard)
production_storage = "\n".join(
    production_part((ROOT / "crates/server-storage/src" / filename).read_text(encoding="utf-8"))
    for filename in ("wizard.rs", "identity.rs", "path.rs", "capacity.rs")
    if (ROOT / "crates/server-storage/src" / filename).is_file()
)

require(
    "server object data and client library are explicitly separate",
    all(token in contract for token in ("SERVER_OBJECT_DATA", "CLIENT_LIBRARY", "synchronized by a client")),
)
require(
    "recommended default is the dedicated managed root",
    '"/var/lib/synveil/storage"' in wizard
    and "/var/lib/synveil/storage" in contract
    and " ".join(contract.split()).find("Where should Synveil store server data?") >= 0,
)
require(
    "durable resumable storage state carries root, storage ID, and root identity",
    all(token in config_model for token in ("PreparingLocal", "ConfiguredLocal", "storage_id", "root_identity"))
    and "begin_storage_preparation" in config_store
    and "complete_storage_preparation" in config_store,
)
require(
    "ConfiguredLocal persists validated capability evidence for existing-only runtime reopen",
    "capabilities: StorageCapabilities" in config_model
    and "valid_local_storage_capabilities" in config_model
    and "open_existing_local_object_store_with_capabilities" in wizard
    and "capabilities.clone()" in wizard,
)
require(
    "ordinary wizard views keep low-level capability evidence out of the UI DTO",
    "pub capabilities:" not in view_model
    and "technical capability" in contract.lower(),
)
require(
    "removable-media status stays explicitly unknown when unqualified",
    "pub enum StorageRemovability" in view_model
    and "removability: StorageRemovability::Unknown" in wizard,
)
require("storage identity uses canonical UUIDv7", "get_version_num() != 7" in config_model and "Uuid::now_v7()" in config_model)
require(
    "bounded strict server-storage identity marker binds both IDs",
    all(token in identity for token in (".synveil-server-storage.json", "deny_unknown_fields", "MAX_MARKER_BYTES", "server_installation_id", "storage_id", "object_layout_version"))
    and "self.server_installation_id == config.server_installation_id" in identity
    and "&self.storage_id == storage_id" in identity,
)
require(
    "existing LocalFilesystemObjectStore remains canonical and keeps its own marker",
    "LocalFilesystemObjectStore" in wizard
    and 'const ROOT_MARKER_NAME: &str = ".synveil-storage-root"' in storage_adapter
    and "initialize_local_object_store_root" in wizard,
)
require(
    "legacy ObjectStore roots are classified for review, not adopted",
    "LegacyObjectStoreNeedsReview" in wizard
    and "no_automatic_legacy_adoption" in wizard
    and "local_store_marker_exists" in wizard,
)
require(
    "unknown non-empty roots are rejected and canary data is preserved in tests",
    "NonEmptyUnknownDirectory" in wizard
    and "an_existing_empty_directory_is_usable_but_unknown_content_is_preserved" in wizard
    and "do not change" in wizard,
)
require(
    "identity mismatch rejects reuse of a replacement root at the same path",
    "RootIdentityMismatch" in wizard
    and "configured_unavailable_root_is_not_replaced_and_relocation_is_rejected" in wizard,
)
require(
    "path exclusions cover config, credentials, PostgreSQL, runtime, home, workspace, and current directory",
    all(token in path_source for token in ('"/etc/synveil"', '"/etc/synveil/credentials"', '"/var/lib/synveil/postgresql"', '"/run"', '"/tmp"', '"/var/tmp"', '"HOME"', "current_dir()", "CARGO_MANIFEST_DIR")),
)
require(
    "client-library overlap uses component-aware path identity and has wizard tests",
    "paths_overlap" in path_source
    and "first.starts_with(second) || second.starts_with(first)" in path_source
    and "configured_client_library_roots_are_excluded_component_wise" in wizard,
)
require(
    "filesystem root and unsafe locations are rejected",
    "canonical == Path::new(\"/\")" in path_source
    and "selected_path_inspection_rejects_root_relative_symlink_wrong_type_and_world_write" in path_source,
)
require(
    "capacity uses native statvfs and checks overflow",
    "libc::statvfs" in (ROOT / "crates/server-storage/src/capacity.rs").read_text(encoding="utf-8")
    and "checked_mul" in (ROOT / "crates/server-storage/src/capacity.rs").read_text(encoding="utf-8"),
)
require(
    "inspection does not create selected storage and capacity/read-only constraints are covered",
    "Read-only inspection" in wizard
    and "recommended_location_and_browse_inspection_do_not_mutate" in wizard
    and "capacity_model_rejects_read_only_unknown_and_insufficient_results" in wizard,
)
require(
    "confirmation binds current config and filesystem evidence before mutation",
    "StalePlan" in wizard
    and "fresh_candidate != plan.candidate" in wizard
    and "begin_storage_preparation" in wizard
    and "concurrent_different_root_selections_commit_at_most_one_root" in wizard,
)
require(
    "durability probe uses the shared capability model and exact probe cleanup",
    "StorageCapability::DurableFsync" in wizard
    and "StorageCapability::AtomicRename" in wizard
    and "run_write_durability_probe" in wizard
    and "remove_exact_file" in wizard,
)
require(
    "managed runtime uses an existing-only opener in both API and worker",
    api.count("open_existing_managed_local_storage") >= 1
    and worker.count("open_existing_managed_local_storage") >= 1
    and "open_existing_local_object_store" in wizard
    and "validate_runtime_capacity" in wizard,
)
require(
    "managed runtime requires ConfiguredLocal and exact root identity",
    "StorageConfiguration::ConfiguredLocal" in runtime_config
    and "same_directory_identity" in wizard
    and "RootIdentityMismatch" in wizard,
)
require(
    "legacy explicit operator ObjectStore behavior remains available",
    "SYNVEIL_OBJECT_ROOT" in api
    and "open_local_object_store" in api
    and "open_local_object_store" in worker,
)
require(
    "no recursive deletion, recursive ownership/mode change, or disk-management commands in production storage code",
    not re.search(r"remove_dir_all|chmod\s+-R|chown\s+-R|\b(initdb|mkfs|fdisk|parted)\b", production_storage),
)
require(
    "P032 contains no service, network, or first-admin provisioning",
    not re.search(r"\b(systemctl|systemd|initdb|create_first_admin|admin_password|SYNVEIL_BIND_ADDR)\b", production_wizard),
)
require(
    "no ordinary relocation implementation exists",
    "StorageRelocationRequiresMigration" in wizard
    and "There is no ordinary storage relocation" in contract
    and not re.search(r"\b(relocate_storage|move_storage_tree|copy_storage_tree)\b", production_storage),
)
require("P032 validator is integrated into docs validation", "validate-server-storage-location.py" in validator_integration)
require(
    "FIRST-RUN-2 remains implementation pending",
    json.loads(read("tests/install-acceptance/scenarios/first-run-2.json")).get("availability") == "IMPLEMENTATION_PENDING",
)
require("roadmap marks only P032 implemented", "Implemented (Prompt032)" in roadmap and "P033–P036" in roadmap)
require("ADR-062 accepted and completion marker recorded", "Accepted / Chấp thuận" in adr and "SYNVEIL_SERVER_STORAGE_LOCATION_READY" in manifest)
require("manifest preserves P028 and inherited P031 run distinctions", "P028" in manifest and "P031" in manifest and "quick-xml 0.38.4" in manifest)
require("guided Host readiness is not claimed", "SYNVEIL_V0_2_GUIDED_SELF_HOSTING_READY" not in manifest and "does not establish `SERVER_READY`" in manifest)

if FAILURES:
    for failure in FAILURES:
        print(f"FAIL: {failure}", file=sys.stderr)
    raise SystemExit(1)

print("PASS: server storage location static invariants")
