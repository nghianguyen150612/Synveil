#!/usr/bin/env python3
"""
Unit tests for clients/ios/Support/validate_ios_sources.py
"""

import unittest
from clients.ios.Support.validate_ios_sources import (
    check_path_and_filename_invariants,
    check_file_content_invariants,
    parse_swift_imports,
)


class TestValidateIOSSources(unittest.TestCase):

    def test_parse_swift_imports(self):
        content = """
        import Foundation
        import SwiftUI
        // import UIKit
        import Security
        """
        imports = parse_swift_imports(content)
        self.assertEqual(imports, {"Foundation", "SwiftUI", "Security"})

    def test_path_and_filename_invariants_allowed(self):
        rel_path = "clients/ios/Domain/ServerProfile.swift"
        violations = check_path_and_filename_invariants(rel_path)
        self.assertEqual(violations, [])

    def test_path_and_filename_invariants_forbidden_ext(self):
        rel_path = "clients/ios/Resources/Secrets.p12"
        violations = check_path_and_filename_invariants(rel_path)
        self.assertTrue(any("Forbidden file extension '.p12'" in v for v in violations))

    def test_path_and_filename_invariants_forbidden_path_substring(self):
        rel_path = "clients/ios/Synveil.xcodeproj/xcuserdata/dev.xcuserdatad"
        violations = check_path_and_filename_invariants(rel_path)
        self.assertTrue(any("Forbidden path substring 'xcuserdata'" in v for v in violations))

    def test_path_and_filename_invariants_unapproved_top_dir(self):
        rel_path = "clients/ios/RandomDir/MyFile.swift"
        violations = check_path_and_filename_invariants(rel_path)
        self.assertTrue(
            any("unapproved top-level directory 'RandomDir'" in v for v in violations)
        )

    def test_domain_layer_allowed(self):
        rel_path = "clients/ios/Domain/ServerProfile.swift"
        content = "import Foundation\n\nstruct ServerProfile {}"
        violations = check_file_content_invariants(rel_path, content)
        self.assertEqual(violations, [])

    def test_domain_layer_forbidden_import_swiftui(self):
        rel_path = "clients/ios/Domain/ServerProfile.swift"
        content = "import Foundation\nimport SwiftUI\n\nstruct ServerProfile {}"
        violations = check_file_content_invariants(rel_path, content)
        self.assertTrue(any("importing forbidden module(s): ['SwiftUI']" in v for v in violations))

    def test_infrastructure_layer_forbidden_import_uikit(self):
        rel_path = "clients/ios/Infrastructure/Network/HTTPClient.swift"
        content = "import Foundation\nimport UIKit"
        violations = check_file_content_invariants(rel_path, content)
        self.assertTrue(any("importing: ['UIKit']" in v for v in violations))

    def test_features_layer_forbidden_import_security(self):
        rel_path = "clients/ios/Features/Auth/LoginView.swift"
        content = "import SwiftUI\nimport Security"
        violations = check_file_content_invariants(rel_path, content)
        self.assertTrue(
            any("directly imports forbidden infrastructure module(s): ['Security']" in v for v in violations)
        )

    def test_rust_ffi_boundary(self):
        # Allowed in RustBridge
        rel_bridge = "clients/ios/Infrastructure/RustBridge/RustAdapter.swift"
        content_bridge = "import Foundation\nimport SynveilCoreFFI"
        self.assertEqual(check_file_content_invariants(rel_bridge, content_bridge), [])

        # Forbidden in Features
        rel_feature = "clients/ios/Features/FileBrowser/FileView.swift"
        content_feature = "import SwiftUI\nimport SynveilCoreFFI"
        violations = check_file_content_invariants(rel_feature, content_feature)
        self.assertTrue(
            any("directly imports raw FFI module(s)" in v for v in violations)
        )

    def test_conflict_markers(self):
        rel_path = "clients/ios/App/SynveilApp.swift"
        content = "<<<<<<< HEAD\nimport SwiftUI\n=======\nimport UIKit\n>>>>>>> branch"
        violations = check_file_content_invariants(rel_path, content)
        self.assertTrue(any("Unresolved git conflict marker" in v for v in violations))

    def test_absolute_path_leakage(self):
        rel_path = "clients/ios/App/SynveilApp.swift"
        content = f"// Config loaded from /Users" + f"/test/config.json"
        violations = check_file_content_invariants(rel_path, content)
        self.assertTrue(any("Absolute machine path leakage" in v for v in violations))


if __name__ == "__main__":
    unittest.main()
