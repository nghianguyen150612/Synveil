#!/usr/bin/env python3
"""Network-free structural gate for the P036 composition boundary."""
from pathlib import Path
import json

ROOT = Path(__file__).resolve().parents[1]

def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")

coordinator = read("crates/server-bootstrap/src/lib.rs")
doc = read("docs/v0.2/END_TO_END_SELF_HOST_WIZARD.md")
manifest = read("docs/v0.2/PROMPT036_MANIFEST.md")
scenario = json.loads(read("tests/install-acceptance/scenarios/first-run-2.json"))

required_code = [
    "enum HostSetupState", "NotStarted", "PreflightPassed", "DependenciesReady",
    "ConfigurationReady", "StorageReady", "ServicesReady",
    "AdminBootstrapRequired", "Ready", "NeedsRepair", "Blocked",
    "ExplicitHostIntentRequired", "StalePlan", "CanonicalOwners",
    "postgres_17_identity_matches", "explicit_admin_login_verified",
    "initial_library_root_coherent", "backend_loopback", "postgres_private",
]
required_docs = [
    "P031 configuration", "P032", "P033", "P034", "P035",
    "No ordinary step", "P037", "P040", "server-side",
]
for needle in required_code:
    assert needle in coordinator, f"missing coordinator contract: {needle}"
for needle in required_docs:
    assert needle in doc, f"missing P036 contract: {needle}"
assert scenario["availability"] == "IMPLEMENTATION_PENDING"
assert "BLOCKED_BY_ENVIRONMENT" in manifest
assert "SYNVEIL_V0_2_GUIDED_SELF_HOSTING_READY" not in coordinator + doc + manifest
assert "Command" not in coordinator and "std::process" not in coordinator
print("end-to-end self-host composition contract: PASS (Phase-E qualification withheld)")
