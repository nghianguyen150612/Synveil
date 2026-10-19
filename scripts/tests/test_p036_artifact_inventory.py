import importlib.util
import json
from pathlib import Path
import unittest
import jsonschema
import tomllib
from unittest.mock import patch
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[2]
PRODUCT_VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
SPEC = importlib.util.spec_from_file_location(
    "p036_artifact_inventory", ROOT / "scripts/validate-p036-production-artifacts.py"
)
inventory = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(inventory)


def valid_inventory():
    artifacts = []
    for platform in inventory.PLATFORMS:
        for component in sorted(inventory.COMPONENTS):
            desktop = component == "desktop"
            source_revision = "f" * 40 if component in {"desktop", "server"} else "a" * 40
            suffix = "deb" if platform == "ubuntu-24.04" else "rpm"
            item = {
                "component": component,
                "version": PRODUCT_VERSION if component in {"desktop", "server"} else ("17.6" if component == "postgresql17" else "2.10.0"),
                "source_revision": source_revision,
                "platform": platform,
                "architecture": "x86_64",
                "artifact": {
                    "path": f"artifacts/{platform}/{component}.{suffix if desktop else 'json'}",
                    "format": suffix if desktop else "runtime_manifest_v2",
                    "size_bytes": 1,
                    "sha256": "b" * 64,
                    "signature_path": f"signatures/{platform}/{component}.sig",
                },
                "provenance": {
                    "path": f"provenance/{platform}/{component}.json",
                    "sha256": "c" * 64,
                    "signature_path": f"provenance/{platform}/{component}.sig",
                    "source_url": "https://example.invalid/source",
                    "source_revision": source_revision,
                },
                "license": {
                    "spdx_id": inventory.EXPECTED_LICENSES[component],
                    "path": f"licenses/{platform}/{component}.LICENSE",
                    "sha256": "d" * 64,
                    "notice_path": f"licenses/{platform}/{component}.NOTICE",
                    "notice_sha256": "e" * 64,
                },
            }
            if not desktop:
                item["artifact"]["payload_root"] = f"artifacts/{platform}/{component}"
                item["artifact"]["path"] = f"artifacts/{platform}/{component}/manifest.json"
                item["provenance"]["path"] = f"artifacts/{platform}/{component}/PROVENANCE"
                item["license"]["path"] = f"artifacts/{platform}/{component}/LICENSE"
                item["license"]["notice_path"] = f"artifacts/{platform}/{component}/NOTICE"
            artifacts.append(item)
    return {
        "schema_version": 1,
        "product": "Synveil",
        "product_version": PRODUCT_VERSION,
        "source_commit": "f" * 40,
        "artifacts": artifacts,
    }


class InventoryContractTests(unittest.TestCase):
    def test_schema_file_is_strict_and_valid_json(self):
        schema = json.loads((ROOT / "acceptance/p036/production-artifacts.schema.json").read_text())
        self.assertEqual(schema["$schema"], "https://json-schema.org/draft/2020-12/schema")
        self.assertFalse(schema["additionalProperties"])
        jsonschema.Draft202012Validator.check_schema(schema)

    def test_versioned_json_schema_validates_and_rejects_unknown_fields(self):
        schema = json.loads((ROOT / "acceptance/p036/production-artifacts.schema.json").read_text())
        validator = jsonschema.Draft202012Validator(schema)
        document = valid_inventory()
        self.assertEqual(list(validator.iter_errors(document)), [])
        document["trust_me"] = True
        self.assertTrue(list(validator.iter_errors(document)))

    def test_complete_platform_component_matrix_is_accepted(self):
        result = inventory.validate_structure(valid_inventory(), "f" * 40, "ubuntu-24.04")
        self.assertEqual(len(result["artifacts"]), 8)

    def test_unbound_source_commit_is_rejected(self):
        data = valid_inventory()
        data["source_commit"] = "0" * 40
        with self.assertRaisesRegex(inventory.InventoryError, "source revision"):
            inventory.validate_structure(data, "f" * 40, None)

    def test_unreleased_or_mismatched_product_version_is_rejected(self):
        data = valid_inventory()
        data["product_version"] = "0.2.0" if PRODUCT_VERSION != "0.2.0" else "0.1.0"
        for entry in data["artifacts"]:
            if entry["component"] in {"desktop", "server"}:
                entry["version"] = data["product_version"]
                entry["source_revision"] = data["source_commit"]
                entry["provenance"]["source_revision"] = data["source_commit"]
        with self.assertRaisesRegex(inventory.InventoryError, "Cargo workspace version"):
            inventory.validate_structure(data, "f" * 40, None)

    def test_missing_component_is_rejected(self):
        data = valid_inventory()
        data["artifacts"].pop()
        with self.assertRaisesRegex(inventory.InventoryError, "versioned P036 JSON schema"):
            inventory.validate_structure(data, "f" * 40, None)

    def test_duplicate_component_and_platform_is_rejected(self):
        data = valid_inventory()
        data["artifacts"][-1] = data["artifacts"][0].copy()
        with self.assertRaisesRegex(inventory.InventoryError, "duplicate component/platform"):
            inventory.validate_structure(data, "f" * 40, None)

    def test_placeholder_version_and_license_are_rejected(self):
        data = valid_inventory()
        postgresql = next(item for item in data["artifacts"] if item["component"] == "postgresql17")
        postgresql["version"] = "latest"
        with self.assertRaisesRegex(inventory.InventoryError, "versioned P036 JSON schema"):
            inventory.validate_structure(data, "f" * 40, None)
        data = valid_inventory()
        data["artifacts"][0]["license"]["spdx_id"] = "Custom"
        with self.assertRaisesRegex(inventory.InventoryError, "versioned P036 JSON schema"):
            inventory.validate_structure(data, "f" * 40, None)

    def test_unknown_inventory_fields_are_rejected(self):
        data = valid_inventory()
        data["trust_me"] = True
        with self.assertRaisesRegex(inventory.InventoryError, "versioned P036 JSON schema"):
            inventory.validate_structure(data, "f" * 40, None)

    def test_noncanonical_and_redirected_artifact_paths_are_rejected(self):
        for value in ("../outside", "artifacts//file", "artifacts/./file", "artifacts\\file"):
            with self.subTest(value=value), self.assertRaises(inventory.InventoryError):
                inventory.relative_path(value, "artifact")

    def test_source_commit_allows_only_release_inventory_and_documentation_followups(self):
        commit = "f" * 40
        for path in (
            "acceptance/p036/production-artifacts.json",
            "acceptance/p036/production-artifacts.json.sig",
            "docs/v0.2/RELEASE_HARDENING_R02_MANIFEST.md",
            "docs/v0.2/RELEASE_HARDENING_R03_MANIFEST.md",
            "docs/v0.2/RELEASE_ARTIFACT_MATRIX.md",
            "docs/v0.2/HOST_RUNTIME_INPUTS.md",
            "docs/v0.2/RELEASE_HARDENING_TRACKER.md",
        ):
            with self.subTest(path=path), patch.object(inventory.subprocess, "run", side_effect=[
                SimpleNamespace(returncode=0, stdout=b""),
                SimpleNamespace(returncode=0, stdout=(path + "\n").encode()),
            ]):
                inventory.verify_source_commit(commit, commit)

    def test_source_commit_rejects_changes_outside_metadata_allowlist(self):
        commit = "f" * 40
        with patch.object(inventory.subprocess, "run", side_effect=[
            SimpleNamespace(returncode=0, stdout=b""),
            SimpleNamespace(returncode=0, stdout=b"scripts/validate-p036-production-artifacts.py\n"),
        ]), self.assertRaisesRegex(inventory.InventoryError, "stale against changed source"):
            inventory.verify_source_commit(commit, commit)


if __name__ == "__main__":
    unittest.main()
