#!/usr/bin/env python3
"""
Synveil iOS v0.1 Source & Repository Invariants Validator

Enforces repository, file path, security, and architectural boundary invariants
defined in ADR-058 and docs/ios/IOS_ARCHITECTURE.md.
"""

import os
import re
import sys
from pathlib import Path

# Directories allowed for Swift production source under clients/ios/
ALLOWED_PRODUCTION_SWIFT_DIRS = {
    "App",
    "Features",
    "Application",
    "Domain",
    "Infrastructure",
    "Extensions",
    "Resources",
}

# Directories allowed for Swift test source under clients/ios/
ALLOWED_TEST_SWIFT_DIRS = {
    "Tests",
}

# Forbidden imports in Domain layer
DOMAIN_FORBIDDEN_IMPORTS = {"SwiftUI", "UIKit", "Security", "GRDB", "SQLite3", "SQLite"}

# Forbidden imports in Infrastructure layer (must remain UI-independent)
INFRASTRUCTURE_FORBIDDEN_IMPORTS = {"SwiftUI", "UIKit"}

# Forbidden direct infrastructure imports in Features layer
FEATURES_FORBIDDEN_IMPORTS = {"Security", "GRDB", "SQLite3", "SQLite"}

# Known raw FFI module names (can be expanded in Phase C / P013+)
RAW_FFI_MODULES = {"synveil_core_ffi", "SynveilCoreFFI", "CBridge", "SynveilRustFFI"}

# Prohibited platform symbols in upper layers (Domain, Application, Features)
FORBIDDEN_NETWORK_SYMBOLS = {
    "URLSession",
    "URLSessionTask",
    "URLSessionConfiguration",
    "URLSessionDataTask",
    "URLSessionUploadTask",
    "URLSessionDownloadTask",
    "HTTPURLResponse",
    "URLSessionDelegate",
}

FORBIDDEN_KEYCHAIN_SYMBOLS = {
    "SecItemAdd",
    "SecItemCopyMatching",
    "SecItemUpdate",
    "SecItemDelete",
}

FORBIDDEN_SQLITE_SYMBOLS = {
    "sqlite3_open",
    "sqlite3_exec",
    "DatabaseQueue",
    "DatabasePool",
}

# Prohibited file extensions / patterns for secrets and build artifacts
FORBIDDEN_FILE_EXTENSIONS = {
    ".p12",
    ".mobileprovision",
    ".provisionprofile",
    ".xcuserstate",
}

FORBIDDEN_PATH_SUBSTRINGS = [
    "xcuserdata",
    "DerivedData",
]

# Conflict markers
CONFLICT_MARKERS = [
    re.compile(r"^<<<<<<< ", re.MULTILINE),
    re.compile(r"^=======\s*$", re.MULTILINE),
    re.compile(r"^>>>>>>> ", re.MULTILINE),
]

# Absolute machine path leakage regex built dynamically to avoid self-matching
_PATH_PREFIXES = ["/Users" + "/", "/home" + "/", r"[A-Za-z]:\\"]
ABSOLUTE_PATH_REGEX = re.compile(r"(?:" + "|".join(_PATH_PREFIXES) + r")")


def check_path_and_filename_invariants(rel_path_str):
    """Checks tracked path strings for user-state leakage, secrets, and path policy."""
    violations = []
    path_obj = Path(rel_path_str)

    # Ignore python cache directories and compiled files
    if "__pycache__" in path_obj.parts or path_obj.suffix in {".pyc", ".pyo"}:
        return violations

    # 1. Xcode user-state & secret file extensions
    ext = path_obj.suffix.lower()
    if ext in FORBIDDEN_FILE_EXTENSIONS:
        violations.append(f"Forbidden file extension '{ext}' in tracked path '{rel_path_str}'")

    # 2. Xcode user-state & build artifact directories
    for substring in FORBIDDEN_PATH_SUBSTRINGS:
        if substring in path_obj.parts or any(substring in part for part in path_obj.parts):
            violations.append(f"Forbidden path substring '{substring}' found in '{rel_path_str}'")

    # 3. Source location policy for Swift files under clients/ios/
    if rel_path_str.startswith("clients/ios/") and ext == ".swift":
        parts = Path(rel_path_str).parts
        # parts[0] == 'clients', parts[1] == 'ios'
        if len(parts) > 2:
            top_dir = parts[2]
            all_allowed = ALLOWED_PRODUCTION_SWIFT_DIRS | ALLOWED_TEST_SWIFT_DIRS | {"Support"}
            if top_dir not in all_allowed:
                violations.append(
                    f"Swift source file '{rel_path_str}' lives in unapproved top-level directory '{top_dir}'. "
                    f"Must be under one of: {sorted(list(all_allowed))}"
                )
        else:
            violations.append(
                f"Swift source file '{rel_path_str}' cannot reside directly in clients/ios/ root."
            )

    return violations


def parse_swift_imports(content):
    """Extracts imported module names from Swift source content."""
    imports = set()
    # Matches import statements including attributes like @testable and import kinds like import class Module.Type
    import_regex = re.compile(
        r"^(?:@\w+\s+)*import\s+(?:(?:class|struct|enum|protocol|typealias|func|var|let)\s+)?([A-Za-z0-9_]+)",
        re.MULTILINE,
    )
    for line in content.splitlines():
        line = line.strip()
        # Skip single line comments
        if line.startswith("//"):
            continue
        match = import_regex.match(line)
        if match:
            imports.add(match.group(1))
    return imports


def strip_comments_and_strings(content):
    """Strips comments and string literals from Swift source code to prevent false positives."""
    # Strip multi-line comments /* ... */
    content = re.sub(r"/\*.*?\*/", "", content, flags=re.DOTALL)
    # Strip single-line comments // ...
    content = re.sub(r"//.*$", "", content, flags=re.MULTILINE)
    # Strip multi-line string literals """..."""
    content = re.sub(r'""".*?"""', '""', content, flags=re.DOTALL)
    # Strip single-line string literals "..."
    content = re.sub(r'"(?:\\.|[^"\\])*"', '""', content)
    return content


def check_file_content_invariants(rel_path_str, content):
    """Checks source file contents for path leakage, conflict markers, and architecture rules."""
    violations = []
    path_obj = Path(rel_path_str)
    ext = path_obj.suffix.lower()

    # Skip python cache files, binary assets
    if "__pycache__" in path_obj.parts or ext in {
        ".pyc",
        ".pyo",
        ".png",
        ".jpg",
        ".jpeg",
        ".xcassets",
        ".car",
        ".zip",
        ".tar",
        ".gz",
    }:
        return violations

    # 1. Conflict markers (check across all text/source/config files)
    for marker_re in CONFLICT_MARKERS:
        if marker_re.search(content):
            violations.append(f"Unresolved git conflict marker found in '{rel_path_str}'")
            break

    # 2. Absolute machine path leakage (exclude documentation / README markdown files)
    if ext != ".md":
        lines = content.splitlines()
        for idx, line in enumerate(lines, 1):
            if ABSOLUTE_PATH_REGEX.search(line):
                violations.append(
                    f"Absolute machine path leakage detected in '{rel_path_str}' line {idx}: {line.strip()}"
                )

    # 3. Architecture & Layer dependency rules for Swift files
    if ext == ".swift" and rel_path_str.startswith("clients/ios/"):
        imports = parse_swift_imports(content)
        stripped_code = strip_comments_and_strings(content)
        parts = path_obj.parts  # e.g. ('clients', 'ios', 'Domain', ...)

        if len(parts) > 2:
            layer = parts[2]

            # Domain layer rules
            if layer == "Domain":
                forbidden_found = imports.intersection(DOMAIN_FORBIDDEN_IMPORTS)
                if forbidden_found:
                    violations.append(
                        f"Domain file '{rel_path_str}' violates layer boundaries by importing forbidden module(s): {sorted(list(forbidden_found))}"
                    )
                # Raw FFI check in Domain
                ffi_found = imports.intersection(RAW_FFI_MODULES)
                if ffi_found:
                    violations.append(
                        f"Domain file '{rel_path_str}' imports raw FFI module(s): {sorted(list(ffi_found))}"
                    )

            # Infrastructure layer rules
            elif layer == "Infrastructure":
                forbidden_found = imports.intersection(INFRASTRUCTURE_FORBIDDEN_IMPORTS)
                if forbidden_found:
                    violations.append(
                        f"Infrastructure file '{rel_path_str}' violates UI-independence by importing: {sorted(list(forbidden_found))}"
                    )

            # Features layer rules
            elif layer == "Features":
                forbidden_found = imports.intersection(FEATURES_FORBIDDEN_IMPORTS)
                if forbidden_found:
                    violations.append(
                        f"Features file '{rel_path_str}' directly imports forbidden infrastructure module(s): {sorted(list(forbidden_found))}"
                    )
                ffi_found = imports.intersection(RAW_FFI_MODULES)
                if ffi_found:
                    violations.append(
                        f"Features file '{rel_path_str}' directly imports raw FFI module(s): {sorted(list(ffi_found))}"
                    )

            # Symbol usage rules for upper layers (Domain, Application, Features)
            if layer in {"Domain", "Application", "Features"}:
                net_symbols = [
                    s
                    for s in FORBIDDEN_NETWORK_SYMBOLS
                    if re.search(r"\b" + re.escape(s) + r"\b", stripped_code)
                ]
                if net_symbols:
                    violations.append(
                        f"{layer} file '{rel_path_str}' directly uses prohibited networking symbol(s): {sorted(net_symbols)}"
                    )

                keychain_symbols = [
                    s
                    for s in FORBIDDEN_KEYCHAIN_SYMBOLS
                    if re.search(r"\b" + re.escape(s) + r"\b", stripped_code)
                ]
                if keychain_symbols:
                    violations.append(
                        f"{layer} file '{rel_path_str}' directly uses prohibited Keychain symbol(s): {sorted(keychain_symbols)}"
                    )

                sqlite_symbols = [
                    s
                    for s in FORBIDDEN_SQLITE_SYMBOLS
                    if re.search(r"\b" + re.escape(s) + r"\b", stripped_code)
                ]
                if sqlite_symbols:
                    violations.append(
                        f"{layer} file '{rel_path_str}' directly uses prohibited SQLite/GRDB symbol(s): {sorted(sqlite_symbols)}"
                    )

            # Raw FFI boundary reservation across all layers outside Infrastructure/RustBridge
            is_rust_bridge = (
                len(parts) >= 4 and layer == "Infrastructure" and parts[3] == "RustBridge"
            )
            is_test = layer == "Tests"
            if not is_rust_bridge:
                ffi_found = imports.intersection(RAW_FFI_MODULES)
                if ffi_found:
                    violations.append(
                        f"File '{rel_path_str}' imports raw FFI module(s) {sorted(list(ffi_found))} outside Infrastructure/RustBridge"
                    )

            # Direct synchronous RustBridgeAdapter prohibition in upper production layers (App, Domain, Application, Features)
            if not is_rust_bridge and not is_test:
                if re.search(r"\bRustBridgeAdapter\b", stripped_code):
                    violations.append(
                        f"Production file '{rel_path_str}' directly uses synchronous primitive 'RustBridgeAdapter' outside Infrastructure/RustBridge. Upper layers must use 'RustBridgeAsyncAdapter' or 'RustBridgeProtocol'."
                    )

            # Direct concrete RustBridgeAsyncAdapter prohibition in Application, Domain, and Features layers
            if layer in {"Application", "Domain", "Features"}:
                if re.search(r"\bRustBridgeAsyncAdapter\b", stripped_code):
                    violations.append(
                        f"{layer} file '{rel_path_str}' directly uses concrete 'RustBridgeAsyncAdapter'. Must depend on 'RustBridgeProtocol' abstraction instead."
                    )

    return violations


def validate_repository(repo_root):
    """Scans the repository directory tree for invariant violations."""
    all_violations = []
    ios_root = Path(repo_root) / "clients" / "ios"

    if not ios_root.exists():
        return [f"iOS root directory not found at {ios_root}"]

    # 1. Scan filesystem paths under clients/ios/
    for root, dirs, files in os.walk(ios_root):
        rel_dir = os.path.relpath(root, repo_root)

        # Check directory path
        dir_violations = check_path_and_filename_invariants(rel_dir)
        all_violations.extend(dir_violations)

        for f in files:
            rel_file_path = os.path.join(rel_dir, f)

            # Path checks
            path_violations = check_path_and_filename_invariants(rel_file_path)
            all_violations.extend(path_violations)

            # Content checks for readable text files
            full_path = os.path.join(root, f)
            if "__pycache__" in Path(rel_file_path).parts or f.endswith((".pyc", ".pyo")):
                continue
            try:
                with open(full_path, "r", encoding="utf-8", errors="replace") as fh:
                    content = fh.read()
                    content_violations = check_file_content_invariants(rel_file_path, content)
                    all_violations.extend(content_violations)
            except Exception as e:
                all_violations.append(f"Failed to read file '{rel_file_path}': {e}")

    return all_violations


def main():
    repo_root = Path(__file__).resolve().parent.parent.parent.parent
    print(f"=== Synveil iOS Static Source Validator ===")
    print(f"Target Repository Root: {repo_root}")

    violations = validate_repository(repo_root)

    if violations:
        print("\n::error:: Static validation failed with the following violation(s):\n")
        for v in violations:
            print(f"  - {v}")
        print(f"\nTotal Violations: {len(violations)}")
        sys.exit(1)
    else:
        print("\n[SUCCESS] All Synveil iOS static source and architecture invariants passed.")
        sys.exit(0)


if __name__ == "__main__":
    main()
