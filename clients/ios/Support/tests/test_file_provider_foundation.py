"""P053 extension target, isolation, metadata, entitlement, and CI contracts."""

import plistlib
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[4]
IOS = ROOT / "clients" / "ios"
EXT = IOS / "Extensions" / "FileProvider"
PROJECT = (IOS / "Synveil.xcodeproj" / "project.pbxproj").read_text()
TARGET_ID = "D05300000000000000000301"
SOURCES_ID = "D05300000000000000000303"


def object_body(identifier: str, comment: str) -> str:
    pattern = rf"(?ms)^\s*{identifier} /\* {re.escape(comment)} \*/ = \{{\n(.*?)^\t\t\}};"
    match = re.search(pattern, PROJECT)
    if not match:
        raise AssertionError(f"Missing Xcode project object {comment}")
    return match.group(1)


class FileProviderFoundationTests(unittest.TestCase):
    def test_extension_target_is_native_embedded_and_depended_on(self):
        target = object_body(TARGET_ID, "SynveilFileProviderExtension")
        self.assertIn('productType = "com.apple.product-type.app-extension";', target)
        self.assertIn("D05300000000000000000303 /* FileProvider Sources */", target)
        self.assertIn("com.synveil.ios.fileprovider", PROJECT)
        self.assertIn("D05300000000000000000302 /* Embed App Extensions */", PROJECT)
        self.assertIn("dstSubfolderSpec = 13;", PROJECT)
        self.assertIn("D0530000000000000000030A /* PBXTargetDependency */", PROJECT)
        self.assertIn("E0530000000000000000021C /* SynveilFileProviderExtension.appex in Embed App Extensions */", PROJECT)

    def test_extension_info_and_app_group_entitlements_match(self):
        info = plistlib.loads((EXT / "Info.plist").read_bytes())
        extension = info["NSExtension"]
        self.assertEqual(extension["NSExtensionPointIdentifier"], "com.apple.fileprovider-nonui")
        self.assertEqual(
            extension["NSExtensionPrincipalClass"],
            "$(PRODUCT_MODULE_NAME).SynveilFileProviderExtension",
        )
        self.assertEqual(
            extension["NSExtensionAttributes"]["NSExtensionFileProviderDocumentGroup"],
            "group.com.synveil.ios",
        )
        app_group = "group.com.synveil.ios"
        extension_entitlements = plistlib.loads((EXT / "FileProvider.entitlements").read_bytes())
        app_entitlements = plistlib.loads((IOS / "App" / "Synveil.entitlements").read_bytes())
        self.assertEqual(extension_entitlements["com.apple.security.application-groups"], [app_group])
        self.assertEqual(app_entitlements["com.apple.security.application-groups"], [app_group])
        self.assertNotIn("keychain-access-groups", extension_entitlements)
        self.assertNotIn("keychain-access-groups", app_entitlements)

    def test_shared_contract_sources_are_separate_from_extension_entrypoint(self):
        app_sources = object_body("A80000012C00000000000001", "Sources")
        extension_sources = object_body(SOURCES_ID, "FileProvider Sources")
        self.assertIn("FileProviderDomainIdentity.swift in Sources", app_sources)
        self.assertIn("FileProviderMetadataContract.swift in Sources", app_sources)
        self.assertNotIn("SynveilFileProviderExtension.swift in Sources", app_sources)
        for filename in [
            "FileProviderDomainIdentity.swift",
            "FileProviderItemIdentity.swift",
            "FileProviderCapability.swift",
            "FileProviderAuthorizationBoundary.swift",
            "FileProviderSharedContainer.swift",
            "FileProviderMetadataContract.swift",
            "FileProviderErrorMapper.swift",
            "FileProviderEnumerator.swift",
            "FileProviderLifecycle.swift",
            "SynveilFileProviderExtension.swift",
        ]:
            self.assertIn(f"{filename} in Sources", extension_sources)

        ext_code = "\n".join(path.read_text() for path in EXT.glob("*.swift"))
        for forbidden in [
            "import SwiftUI",
            "import UIKit",
            "import Security",
            "import SQLite3",
            "URLSession.shared",
            "AppDependencyContainer(",
            "BackgroundUploadApplicationBootstrap",
            "MutationQueueSQLiteStore(",
        ]:
            self.assertNotIn(forbidden, ext_code)
        self.assertNotIn("synveil_ios_ffi", extension_sources)
        self.assertNotIn("OTHER_LDFLAGS", object_body("D05300000000000000000307", "Debug"))

    def test_fail_closed_contract_keeps_registration_and_network_disabled(self):
        capability = (EXT / "FileProviderCapability.swift").read_text()
        extension = (EXT / "SynveilFileProviderExtension.swift").read_text()
        mapper = (EXT / "FileProviderErrorMapper.swift").read_text()
        self.assertIn("domainRegistrationEnabled: false", capability)
        self.assertIn("metadataPublicationEnabled: false", capability)
        self.assertIn("authenticatedNetworkEnabled: false", capability)
        self.assertIn("mutationEnabled: false", capability)
        self.assertIn("throw FileProviderErrorMapper.unavailable()", extension)
        self.assertIn("FileProviderErrorMapper.map(error)", extension)
        self.assertIn("FileProviderErrorMapper.unsupported()", extension)
        self.assertIn("code = .notAuthenticated", mapper)
        self.assertIn("code = .cannotSynchronize", mapper)

    def test_build_workflow_checks_embedded_extension_product(self):
        workflow = (ROOT / ".github" / "workflows" / "ios-build.yml").read_text()
        self.assertIn("xcodebuild", workflow)
        self.assertIn("SynveilFileProviderExtension.appex", workflow)
        self.assertIn("PlugIns", workflow)
        self.assertIn("Info.plist", workflow)

    def test_product_contract_preserves_v01_deferred_classification(self):
        product = (ROOT / "docs" / "ios" / "IOS_V0_1_PRODUCT_CONTRACT.md").read_text()
        architecture = (ROOT / "docs" / "ios" / "IOS_ARCHITECTURE.md").read_text()
        self.assertIn("IOS_DEFERRED_AFTER_V0_1", product)
        self.assertIn("owner-authorized post-baseline workstream", architecture)
        self.assertIn("P053", architecture)
        self.assertIn("production domain registration remains disabled", architecture)


if __name__ == "__main__":
    unittest.main()
