"""P055 source membership, materialization and content storage safety checks."""
import unittest
from pathlib import Path

from test_file_provider_foundation import EXT, IOS, PROJECT, object_body, SOURCES_ID


class FileProviderMaterializationContracts(unittest.TestCase):
    def test_shared_content_sources_build_in_host_and_extension(self):
        host = object_body("A80000012C00000000000001", "Sources")
        extension = object_body(SOURCES_ID, "FileProvider Sources")
        for name in ["FileProviderContentManifest.swift", "FileProviderContentStore.swift"]:
            self.assertIn(f"{name} in Sources", host)
            self.assertIn(f"{name} in Sources", extension)
        for name in ["FileProviderVerifiedContentSource.swift", "FileProviderVerifiedContentPublicationCoordinator.swift"]:
            self.assertIn(f"{name} in Sources", host)
            self.assertNotIn(f"{name} in Sources", extension)
        self.assertIn("FileProviderMaterializationTests.swift in Sources", PROJECT)

    def test_replicated_entrypoint_implements_fetch_contents(self):
        source = (EXT / "SynveilFileProviderExtension.swift").read_text()
        self.assertIn("func fetchContents(", source)
        self.assertIn("verifyAndAccessContent(", source)
        self.assertIn("requestedVersion", source)
        self.assertIn("temporaryCopyURL", source)
        self.assertIn("FileProviderErrorMapper.map(error)", source)

    def test_item_capabilities_allow_reading_only_for_files(self):
        source = (EXT / "SynveilFileProviderItem.swift").read_text()
        self.assertIn("case .file:", source)
        self.assertIn("[.allowsReading]", source)
        self.assertIn("[.allowsContentEnumerating]", source)

    def test_extension_never_imports_host_data_or_session_owners(self):
        source = "\n".join(path.read_text() for path in EXT.glob("*.swift"))
        for forbidden in ["import SQLite3", "import Security", "import UIKit", "URLSession(",
                          "DeviceBearer(", "SessionController(", "AppDependencyContainer(",
                          "NSFileProviderManager.add("]:
            self.assertNotIn(forbidden, source)

    def test_fail_closed_mutations_remain_unsupported(self):
        source = (EXT / "SynveilFileProviderExtension.swift").read_text()
        self.assertEqual(source.count("FileProviderErrorMapper.unsupported()"), 3)
