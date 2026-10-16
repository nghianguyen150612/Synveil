"""P054 source membership, native callback and storage isolation checks."""
import unittest
from pathlib import Path

from test_file_provider_foundation import EXT, IOS, PROJECT, object_body, SOURCES_ID


class FileProviderEnumerationContracts(unittest.TestCase):
    def test_shared_generation_sources_build_in_host_and_extension(self):
        host = object_body("A80000012C00000000000001", "Sources")
        extension = object_body(SOURCES_ID, "FileProvider Sources")
        for name in ["FileProviderGenerationContract.swift", "FileProviderGenerationStore.swift",
                     "FileProviderMetadataReader.swift", "FileProviderMetadataSignaling.swift",
                     "SynveilFileProviderItem.swift", "FileProviderEnumerator.swift"]:
            self.assertIn(f"{name} in Sources", host)
            self.assertIn(f"{name} in Sources", extension)
        self.assertIn("FileProviderMetadataPublicationCoordinator.swift in Sources", host)
        self.assertNotIn("FileProviderMetadataPublicationCoordinator.swift in Sources", extension)
        self.assertIn("FileProviderEnumerationTests.swift in Sources", PROJECT)

    def test_replicated_entrypoint_uses_real_metadata_adapter(self):
        source = (EXT / "SynveilFileProviderExtension.swift").read_text()
        self.assertIn("NSFileProviderReplicatedExtension", source)
        self.assertIn("SynveilFileProviderEnumerator(", source)
        self.assertIn("view.lookup(identifier.rawValue)", source)
        self.assertIn("completionHandler(nil, nil, FileProviderErrorMapper.unavailable())", source)
        self.assertEqual(source.count("FileProviderErrorMapper.unsupported()"), 3)

    def test_extension_never_imports_host_data_or_session_owners(self):
        source = "\n".join(path.read_text() for path in EXT.glob("*.swift"))
        for forbidden in ["import SQLite3", "import Security", "import UIKit", "URLSession(",
                          "DeviceBearer(", "SessionController(", "AppDependencyContainer(",
                          "NSFileProviderManager.add("]:
            self.assertNotIn(forbidden, source)

    def test_tokens_and_journal_have_explicit_bounds(self):
        contract = (EXT / "FileProviderGenerationContract.swift").read_text()
        self.assertIn("data.count == 162", contract)
        self.assertIn("retainedGenerations = 3", contract)
        self.assertIn("schemaVersion", contract)
        reader = (EXT / "FileProviderMetadataReader.swift").read_text()
        self.assertIn("pageSize = 100", reader)
        self.assertIn("cursor.authorization == manifest.authorization.authorizationGeneration", reader)
        self.assertIn("globallyLive[$0] == nil", reader)

    def test_commit_and_revocation_share_cross_process_fence(self):
        store = (EXT / "FileProviderGenerationStore.swift").read_text()
        self.assertIn("LOCK_NB", store)
        self.assertIn("O_NOFOLLOW", store)
        self.assertIn("handle.synchronize()", store)
        self.assertIn("fsync(directoryFD)", store)
        self.assertIn("beforeCommit", store)
        self.assertIn("existingRecords", store)
        self.assertIn("func revoke(", store)
