import os
import shutil
import tempfile
import unittest

from scripts.validate_ios_rust_artifact import validate_artifact_bundle


class TestValidateIosRustArtifact(unittest.TestCase):
    def setUp(self):
        self.test_dir = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self.test_dir)

    def _create_mock_file(self, rel_path, content=b"dummy"):
        full_path = os.path.join(self.test_dir, rel_path)
        os.makedirs(os.path.dirname(full_path), exist_ok=True)
        with open(full_path, "wb") as f:
            f.write(content)

    def test_valid_bundle_without_x86_64(self):
        hdr_bytes = b"/* header */\ntypedef struct SynveilFfiBuffer { int x; } SynveilFfiBuffer;\nuint32_t synveil_ffi_buffer_release(struct SynveilFfiBuffer *b);\nuint32_t synveil_ffi_enrollment_secret_validate();\nuint32_t synveil_ffi_device_credential_validate();\nuint32_t synveil_ffi_library_id_validate();\nuint32_t synveil_ffi_node_id_validate();\nuint32_t synveil_ffi_logical_name_validate();\nuint32_t synveil_ffi_sha256_parse();\nuint32_t synveil_ffi_sha256_format();\n"
        self._create_mock_file("device/arm64/libsynveil_ios_ffi.a", b"dev_arm64")
        self._create_mock_file("simulator/arm64/libsynveil_ios_ffi.a", b"sim_arm64")
        self._create_mock_file("include/synveil_ios_ffi.h", hdr_bytes)

        # Create SHA256SUMS
        import hashlib
        h_dev = hashlib.sha256(b"dev_arm64").hexdigest()
        h_sim = hashlib.sha256(b"sim_arm64").hexdigest()
        h_hdr = hashlib.sha256(hdr_bytes).hexdigest()

        manifest = {
            "schema_version": 1,
            "package_name": "synveil-ios-ffi",
            "package_version": "0.1.0",
            "artifact_profile": "release",
            "minimum_ios_deployment_target": "17.0",
            "rust_toolchain_version": "rustc 1.94.0",
            "cargo_version": "cargo 1.94.0",
            "source_commit_sha": "abc1234",
            "c_abi_export_status": "MODEL_MAPPING_P020",
            "c_abi_exports": [
                "synveil_ffi_abi_version",
                "synveil_ffi_buffer_release",
                "synveil_ffi_device_credential_validate",
                "synveil_ffi_enrollment_secret_validate",
                "synveil_ffi_library_id_validate",
                "synveil_ffi_logical_name_validate",
                "synveil_ffi_node_id_validate",
                "synveil_ffi_sha256_format",
                "synveil_ffi_sha256_parse",
                "synveil_ffi_validate_abi_version",
            ],
            "ffi_status_model": "P017_STABLE_UINT32",
            "ffi_memory_model": "P018_RUST_OWNED_BUFFER",
            "ffi_concurrency_model": "P019_SWIFT_MANAGED_SYNC_RUST",
            "ffi_model_mapping": "P020_AUTH_FILE_PRIMITIVES",
            "rust_bridge_protocol": "P020_APPLICATION_SERVICE_BOUNDARY",
            "rust_async_runtime": "NONE",
            "callback_abi": "NONE",
            "cancellation_model": "P019_SWIFT_COOPERATIVE_NO_MID_FFI_INTERRUPT",
            "cbindgen_status": "ACTIVE_P017",
            "header_status": "GENERATED_CBINDGEN_P017",
            "variants": [
                {
                    "relative_path": "device/arm64/libsynveil_ios_ffi.a",
                    "rust_target_triple": "aarch64-apple-ios",
                    "apple_platform_role": "device",
                    "architecture": "arm64",
                    "size_bytes": 9,
                    "sha256": h_dev,
                },
                {
                    "relative_path": "simulator/arm64/libsynveil_ios_ffi.a",
                    "rust_target_triple": "aarch64-apple-ios-sim",
                    "apple_platform_role": "simulator",
                    "architecture": "arm64",
                    "size_bytes": 9,
                    "sha256": h_sim,
                },
            ],
        }

        import json
        manifest_bytes = json.dumps(manifest, indent=2).encode("utf-8")
        self._create_mock_file("manifest.json", manifest_bytes)
        h_man = hashlib.sha256(manifest_bytes).hexdigest()

        sums_content = f"{h_dev}  device/arm64/libsynveil_ios_ffi.a\n{h_hdr}  include/synveil_ios_ffi.h\n{h_sim}  simulator/arm64/libsynveil_ios_ffi.a\n{h_man}  manifest.json\n"
        self._create_mock_file("SHA256SUMS", sums_content.encode("utf-8"))

        # Should pass validation
        validate_artifact_bundle(self.test_dir)

    def test_unexpected_file_fails(self):
        hdr_bytes = b"/* header */\ntypedef struct SynveilFfiBuffer { int x; } SynveilFfiBuffer;\nuint32_t synveil_ffi_buffer_release(struct SynveilFfiBuffer *b);\nuint32_t synveil_ffi_enrollment_secret_validate();\nuint32_t synveil_ffi_device_credential_validate();\nuint32_t synveil_ffi_library_id_validate();\nuint32_t synveil_ffi_node_id_validate();\nuint32_t synveil_ffi_logical_name_validate();\nuint32_t synveil_ffi_sha256_parse();\nuint32_t synveil_ffi_sha256_format();\n"
        self._create_mock_file("device/arm64/libsynveil_ios_ffi.a", b"dev_arm64")
        self._create_mock_file("simulator/arm64/libsynveil_ios_ffi.a", b"sim_arm64")
        self._create_mock_file("include/synveil_ios_ffi.h", hdr_bytes)
        self._create_mock_file("unexpected_junk.tmp", b"junk")

        manifest = {
            "schema_version": 1,
            "package_name": "synveil-ios-ffi",
            "artifact_profile": "release",
            "c_abi_export_status": "MODEL_MAPPING_P020",
            "c_abi_exports": [
                "synveil_ffi_abi_version",
                "synveil_ffi_buffer_release",
                "synveil_ffi_device_credential_validate",
                "synveil_ffi_enrollment_secret_validate",
                "synveil_ffi_library_id_validate",
                "synveil_ffi_logical_name_validate",
                "synveil_ffi_node_id_validate",
                "synveil_ffi_sha256_format",
                "synveil_ffi_sha256_parse",
                "synveil_ffi_validate_abi_version",
            ],
            "ffi_status_model": "P017_STABLE_UINT32",
            "ffi_memory_model": "P018_RUST_OWNED_BUFFER",
            "ffi_concurrency_model": "P019_SWIFT_MANAGED_SYNC_RUST",
            "ffi_model_mapping": "P020_AUTH_FILE_PRIMITIVES",
            "rust_bridge_protocol": "P020_APPLICATION_SERVICE_BOUNDARY",
            "rust_async_runtime": "NONE",
            "callback_abi": "NONE",
            "cancellation_model": "P019_SWIFT_COOPERATIVE_NO_MID_FFI_INTERRUPT",
            "header_status": "GENERATED_CBINDGEN_P017",
            "variants": [{"relative_path": "device/arm64/libsynveil_ios_ffi.a", "size_bytes": 9}],
        }
        import json
        self._create_mock_file("manifest.json", json.dumps(manifest).encode("utf-8"))
        self._create_mock_file("SHA256SUMS", b"dummy SHA256SUMS")

        with self.assertRaises(ValueError) as ctx:
            validate_artifact_bundle(self.test_dir)
        self.assertIn("Closed file set validation failed", str(ctx.exception))

    def test_stale_p017_manifest_fails(self):
        hdr_bytes = b"/* header */\ntypedef struct SynveilFfiBuffer { int x; } SynveilFfiBuffer;\nuint32_t synveil_ffi_buffer_release(struct SynveilFfiBuffer *b);\nuint32_t synveil_ffi_sha256_parse();\nuint32_t synveil_ffi_sha256_format();\n"
        self._create_mock_file("device/arm64/libsynveil_ios_ffi.a", b"dev_arm64")
        self._create_mock_file("simulator/arm64/libsynveil_ios_ffi.a", b"sim_arm64")
        self._create_mock_file("include/synveil_ios_ffi.h", hdr_bytes)

        manifest = {
            "schema_version": 1,
            "package_name": "synveil-ios-ffi",
            "artifact_profile": "release",
            "c_abi_export_status": "STATUS_MODEL_P017",
            "c_abi_exports": ["synveil_ffi_abi_version", "synveil_ffi_validate_abi_version"],
            "ffi_status_model": "P017_STABLE_UINT32",
            "header_status": "GENERATED_CBINDGEN_P017",
            "variants": [{"relative_path": "device/arm64/libsynveil_ios_ffi.a", "size_bytes": 9}],
        }
        import json
        self._create_mock_file("manifest.json", json.dumps(manifest).encode("utf-8"))
        self._create_mock_file("SHA256SUMS", b"dummy SHA256SUMS")

        with self.assertRaises(ValueError) as ctx:
            validate_artifact_bundle(self.test_dir)
        self.assertIn("c_abi_export_status", str(ctx.exception))

    def test_invalid_concurrency_metadata_fails(self):
        hdr_bytes = b"/* header */\ntypedef struct SynveilFfiBuffer { int x; } SynveilFfiBuffer;\nuint32_t synveil_ffi_buffer_release(struct SynveilFfiBuffer *b);\nuint32_t synveil_ffi_enrollment_secret_validate();\nuint32_t synveil_ffi_device_credential_validate();\nuint32_t synveil_ffi_library_id_validate();\nuint32_t synveil_ffi_node_id_validate();\nuint32_t synveil_ffi_logical_name_validate();\nuint32_t synveil_ffi_sha256_parse();\nuint32_t synveil_ffi_sha256_format();\n"
        self._create_mock_file("device/arm64/libsynveil_ios_ffi.a", b"dev_arm64")
        self._create_mock_file("simulator/arm64/libsynveil_ios_ffi.a", b"sim_arm64")
        self._create_mock_file("include/synveil_ios_ffi.h", hdr_bytes)

        manifest = {
            "schema_version": 1,
            "package_name": "synveil-ios-ffi",
            "artifact_profile": "release",
            "c_abi_export_status": "MODEL_MAPPING_P020",
            "c_abi_exports": [
                "synveil_ffi_abi_version",
                "synveil_ffi_buffer_release",
                "synveil_ffi_device_credential_validate",
                "synveil_ffi_enrollment_secret_validate",
                "synveil_ffi_library_id_validate",
                "synveil_ffi_logical_name_validate",
                "synveil_ffi_node_id_validate",
                "synveil_ffi_sha256_format",
                "synveil_ffi_sha256_parse",
                "synveil_ffi_validate_abi_version",
            ],
            "ffi_status_model": "P017_STABLE_UINT32",
            "ffi_memory_model": "P018_RUST_OWNED_BUFFER",
            "ffi_concurrency_model": "P019_SWIFT_MANAGED_SYNC_RUST",
            "ffi_model_mapping": "P020_AUTH_FILE_PRIMITIVES",
            "rust_bridge_protocol": "P020_APPLICATION_SERVICE_BOUNDARY",
            "rust_async_runtime": "TOKIO",
            "callback_abi": "NONE",
            "cancellation_model": "P019_SWIFT_COOPERATIVE_NO_MID_FFI_INTERRUPT",
            "header_status": "GENERATED_CBINDGEN_P017",
            "variants": [{"relative_path": "device/arm64/libsynveil_ios_ffi.a", "size_bytes": 9}],
        }
        import json
        self._create_mock_file("manifest.json", json.dumps(manifest).encode("utf-8"))
        self._create_mock_file("SHA256SUMS", b"dummy SHA256SUMS")

        with self.assertRaises(ValueError) as ctx:
            validate_artifact_bundle(self.test_dir)
        self.assertIn("rust_async_runtime", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
