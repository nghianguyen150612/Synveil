#!/usr/bin/env python3
"""
Unit tests for clients/ios/Support/validate_ios_sources.py
"""

import unittest
from clients.ios.Support.validate_ios_sources import (
    check_path_and_filename_invariants,
    check_file_content_invariants,
    parse_swift_imports,
    strip_comments_and_strings,
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

    def test_strip_comments_and_strings(self):
        content = """
        // URLSession.shared should be stripped
        /* URLSessionTask multi-line */
        let msg = "URLSession is safe in strings"
        let session = URLSession.shared
        """
        stripped = strip_comments_and_strings(content)
        self.assertNotIn("// URLSession.shared", stripped)
        self.assertNotIn("URLSessionTask multi-line", stripped)
        self.assertNotIn("URLSession is safe in strings", stripped)
        self.assertIn("URLSession.shared", stripped)

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

    def test_forbidden_network_symbol_usage_rejected_in_upper_layers(self):
        # Features layer rejects URLSession
        rel_feature = "clients/ios/Features/Onboarding/OnboardingView.swift"
        content_feature = "import SwiftUI\n\nlet session = URLSession.shared"
        violations_feature = check_file_content_invariants(rel_feature, content_feature)
        self.assertTrue(any("directly uses prohibited networking symbol(s)" in v for v in violations_feature))

        # Application layer rejects URLSession
        rel_app = "clients/ios/Application/Sync/SyncCoordinator.swift"
        content_app = "import Foundation\n\nlet task: URLSessionDataTask"
        violations_app = check_file_content_invariants(rel_app, content_app)
        self.assertTrue(any("directly uses prohibited networking symbol(s)" in v for v in violations_app))

        # Domain layer rejects URLSession
        rel_domain = "clients/ios/Domain/Services/Service.swift"
        content_domain = "import Foundation\n\nfunc fetch(s: URLSession)"
        violations_domain = check_file_content_invariants(rel_domain, content_domain)
        self.assertTrue(any("directly uses prohibited networking symbol(s)" in v for v in violations_domain))

    def test_forbidden_network_symbol_allowed_in_infrastructure_and_comments(self):
        # Allowed in Infrastructure/Network
        rel_infra = "clients/ios/Infrastructure/Network/URLSessionHTTPTransport.swift"
        content_infra = "import Foundation\n\nlet session = URLSession.shared"
        self.assertEqual(check_file_content_invariants(rel_infra, content_infra), [])

        # Allowed in comments or string literals in Application
        rel_app_doc = "clients/ios/Application/Configuration/AppOrchestrator.swift"
        content_app_doc = 'import Foundation\n\n// Concrete implementation uses URLSession\nlet desc = "URLSession transport"'
        self.assertEqual(check_file_content_invariants(rel_app_doc, content_app_doc), [])

    def test_transport_protocol_abstraction_allowed_in_upper_layers(self):
        rel_app = "clients/ios/Application/Configuration/AppOrchestrator.swift"
        content_app = "import Foundation\n\nstruct AppOrchestrator {\n  let transport: HTTPTransportProtocol\n}"
        self.assertEqual(check_file_content_invariants(rel_app, content_app), [])

    def test_rust_ffi_boundary(self):
        # Allowed in RustBridge (SynveilRustFFI)
        rel_bridge = "clients/ios/Infrastructure/RustBridge/RustBridgeAdapter.swift"
        content_bridge = "import Foundation\nimport SynveilRustFFI\n\nfunc check() { _ = synveil_ffi_abi_version() }"
        self.assertEqual(check_file_content_invariants(rel_bridge, content_bridge), [])

        # Forbidden in Domain
        rel_domain = "clients/ios/Domain/Configuration/ServerEndpoint.swift"
        content_domain = "import Foundation\nimport SynveilRustFFI"
        violations_domain = check_file_content_invariants(rel_domain, content_domain)
        self.assertTrue(
            any("imports raw FFI module(s)" in v for v in violations_domain)
        )

        # Forbidden in Application
        rel_app = "clients/ios/Application/Configuration/AppOrchestrator.swift"
        content_app = "import Foundation\nimport SynveilRustFFI"
        violations_app = check_file_content_invariants(rel_app, content_app)
        self.assertTrue(
            any("imports raw FFI module(s)" in v for v in violations_app)
        )

        # Forbidden in Features
        rel_feature = "clients/ios/Features/FileBrowser/FileView.swift"
        content_feature = "import SwiftUI\nimport SynveilRustFFI"
        violations_feature = check_file_content_invariants(rel_feature, content_feature)
        self.assertTrue(
            any("imports raw FFI module(s)" in v for v in violations_feature)
        )

        # Comments/strings mentioning SynveilRustFFI do not cause false positives
        rel_comment = "clients/ios/Domain/Configuration/ServerEndpoint.swift"
        content_comment = '// SynveilRustFFI module is restricted to RustBridge\nlet doc = "SynveilRustFFI import"'
        self.assertEqual(check_file_content_invariants(rel_comment, content_comment), [])

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
