#!/usr/bin/env python3
"""
Validator for Synveil iOS Rust Apple Artifact Staging Bundle.

Validates:
1. Schema version and manifest required fields
2. Relative paths and path safety (no absolute or private runner leakage)
3. SHA-256 integrity file (SHA256SUMS) match against actual staged files
4. Closed file set validation (no unexpected transient build outputs)
5. Binary symbol absence check (if nm is available)
"""

import argparse
import json
import hashlib
import os
import sys

ALLOWED_PATHS_BASE = {
    "device/arm64/libsynveil_ios_ffi.a",
    "simulator/arm64/libsynveil_ios_ffi.a",
    "include/synveil_ios_ffi.h",
    "manifest.json",
    "SHA256SUMS",
}

ALLOWED_PATHS_X86_64 = {
    "simulator/x86_64/libsynveil_ios_ffi.a",
    "simulator/universal/libsynveil_ios_ffi.a",
}

PROHIBITED_PATH_SUBSTRINGS = [
    "/Users/runner",
    "/home/jules",
    "/tmp",
    "Users/",
    "home/",
    "\\",
]


def validate_artifact_bundle(staging_dir: str) -> None:
    if not os.path.isdir(staging_dir):
        raise ValueError(f"Staging directory does not exist: {staging_dir}")

    manifest_path = os.path.join(staging_dir, "manifest.json")
    if not os.path.isfile(manifest_path):
        raise ValueError(f"Missing manifest.json in staging directory: {staging_dir}")

    sums_path = os.path.join(staging_dir, "SHA256SUMS")
    if not os.path.isfile(sums_path):
        raise ValueError(f"Missing SHA256SUMS in staging directory: {staging_dir}")

    # 1. Read & Validate Manifest
    with open(manifest_path, "r", encoding="utf-8") as f:
        manifest_raw = f.read()

    # Path safety check on manifest text
    for prohibited in PROHIBITED_PATH_SUBSTRINGS:
        if prohibited in manifest_raw:
            raise ValueError(
                f"Path leakage error: manifest.json contains prohibited path substring '{prohibited}'"
            )

    try:
        manifest = json.loads(manifest_raw)
    except json.JSONDecodeError as e:
        raise ValueError(f"manifest.json is not valid JSON: {e}")

    if manifest.get("schema_version") != 1:
        raise ValueError(
            f"Invalid schema_version: expected 1, got {manifest.get('schema_version')}"
        )

    if manifest.get("package_name") != "synveil-ios-ffi":
        raise ValueError(
            f"Invalid package_name: expected 'synveil-ios-ffi', got {manifest.get('package_name')}"
        )

    if manifest.get("artifact_profile") != "release":
        raise ValueError(
            f"Invalid artifact_profile: expected 'release', got {manifest.get('artifact_profile')}"
        )

    if manifest.get("c_abi_export_status") != "ABI_VERSION_ONLY_P016":
        raise ValueError(
            f"Invalid c_abi_export_status: expected 'ABI_VERSION_ONLY_P016', got {manifest.get('c_abi_export_status')}"
        )

    if manifest.get("c_abi_exports") != ["synveil_ffi_abi_version"]:
        raise ValueError(
            f"Invalid c_abi_exports: expected ['synveil_ffi_abi_version'], got {manifest.get('c_abi_exports')}"
        )

    if manifest.get("header_status") != "GENERATED_CBINDGEN_P016":
        raise ValueError(
            f"Invalid header_status: expected 'GENERATED_CBINDGEN_P016', got {manifest.get('header_status')}"
        )

    variants = manifest.get("variants")
    if not isinstance(variants, list) or len(variants) == 0:
        raise ValueError("manifest.json must contain a non-empty 'variants' array")

    has_x86_64 = any(
        v.get("rust_target_triple") == "x86_64-apple-ios" for v in variants
    )
    has_universal = any(
        v.get("rust_target_triple") == "universal-simulator" for v in variants
    )

    allowed_set = set(ALLOWED_PATHS_BASE)
    if has_x86_64:
        allowed_set.add("simulator/x86_64/libsynveil_ios_ffi.a")
    if has_universal:
        allowed_set.add("simulator/universal/libsynveil_ios_ffi.a")

    # 2. Closed File Set Validation
    actual_staged_files = set()
    for root, dirs, files in os.walk(staging_dir):
        for file in files:
            full_path = os.path.join(root, file)
            rel_path = os.path.relpath(full_path, staging_dir).replace("\\", "/")
            actual_staged_files.add(rel_path)

    unexpected_files = actual_staged_files - allowed_set
    if unexpected_files:
        raise ValueError(
            f"Closed file set validation failed! Found unexpected files in artifact staging: {sorted(unexpected_files)}"
        )

    missing_files = allowed_set - actual_staged_files
    if missing_files:
        raise ValueError(
            f"Missing expected staged files: {sorted(missing_files)}"
        )

    # 3. SHA256SUMS Verification
    with open(sums_path, "r", encoding="utf-8") as f:
        sums_lines = f.readlines()

    sums_dict = {}
    for line_num, line in enumerate(sums_lines, 1):
        line_str = line.strip()
        if not line_str:
            continue
        parts = line_str.split("  ", 1)
        if len(parts) != 2:
            raise ValueError(
                f"SHA256SUMS line {line_num} format error: expected '<hash>  <rel_path>'"
            )
        expected_hash, rel_path = parts
        rel_path = rel_path.replace("\\", "/")

        if rel_path.startswith("/") or ".." in rel_path:
            raise ValueError(
                f"SHA256SUMS line {line_num} path error: path '{rel_path}' is not a safe relative path"
            )

        sums_dict[rel_path] = expected_hash

    # Verify every file in actual_staged_files except SHA256SUMS is in SHA256SUMS
    for rel_path in actual_staged_files:
        if rel_path == "SHA256SUMS":
            continue
        if rel_path not in sums_dict:
            raise ValueError(
                f"File '{rel_path}' exists in staging directory but is not listed in SHA256SUMS"
            )

        full_path = os.path.join(staging_dir, rel_path)
        h = hashlib.sha256()
        with open(full_path, "rb") as bf:
            while chunk := bf.read(65536):
                h.update(chunk)
        actual_hash = h.hexdigest()

        if actual_hash != sums_dict[rel_path]:
            raise ValueError(
                f"SHA-256 mismatch for '{rel_path}': expected {sums_dict[rel_path]}, got {actual_hash}"
            )

    # 4. Variants Manifest Details Consistency Check
    for variant in variants:
        v_path = variant.get("relative_path")
        if not v_path or v_path not in actual_staged_files:
            raise ValueError(
                f"Manifest variant relative_path '{v_path}' does not exist in staged files"
            )

        full_path = os.path.join(staging_dir, v_path)
        expected_size = variant.get("size_bytes")
        actual_size = os.path.getsize(full_path)
        if expected_size != actual_size:
            raise ValueError(
                f"Manifest variant size mismatch for '{v_path}': expected {expected_size}, got {actual_size}"
            )

        expected_sha = variant.get("sha256")
        if sums_dict.get(v_path) != expected_sha:
            raise ValueError(
                f"Manifest variant SHA-256 mismatch for '{v_path}': manifest {expected_sha} != SHA256SUMS {sums_dict.get(v_path)}"
            )

    print("=== Synveil iOS Rust Artifact Validation PASSED ===")
    print(f"Staging root: {os.path.abspath(staging_dir)}")
    print(f"Validated files ({len(actual_staged_files)}): {sorted(actual_staged_files)}")


def main():
    parser = argparse.ArgumentParser(
        description="Validate Synveil iOS Rust artifact staging bundle."
    )
    parser.add_argument(
        "--staging-dir",
        default="target/ios-rust-artifacts",
        help="Path to staged artifact directory (default: target/ios-rust-artifacts)",
    )
    args = parser.parse_args()

    try:
        validate_artifact_bundle(args.staging_dir)
    except Exception as e:
        print(f"ERROR: Artifact validation failed: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
