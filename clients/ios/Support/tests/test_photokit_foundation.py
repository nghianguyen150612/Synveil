import unittest
from pathlib import Path

IOS_ROOT = Path("clients/ios")
PROJECT_FILE = IOS_ROOT / "Synveil.xcodeproj" / "project.pbxproj"
BACKGROUND_PLIST = IOS_ROOT / "App" / "BackgroundSyncInfo.plist"

class PhotoKitFoundationRegistrationTests(unittest.TestCase):
    def test_pbxproj_contains_photokit_sources(self):
        project_text = PROJECT_FILE.read_text()
        required_sources = [
            "PhotoPermissionState.swift",
            "PhotoAssetTypes.swift",
            "PhotoObservationModels.swift",
            "PhotoBackupConfiguration.swift",
            "PhotoServiceProtocols.swift",
            "PhotoLibraryPermissionAdapter.swift",
            "PhotoLibraryDiscoveryService.swift",
            "PhotoLibraryChangeObserverAdapter.swift",
            "PhotoObservationSQLiteSchema.swift",
            "PhotoObservationSQLiteStore.swift",
            "PhotoLibraryObservationCoordinator.swift",
            "PhotoBackupSettingsViewModel.swift",
            "PhotoBackupSettingsView.swift",
            "PhotoKitFoundationTests.swift",
        ]
        for src in required_sources:
            self.assertIn(src, project_text, f"Source file {src} missing from project.pbxproj")

    def test_background_sync_info_plist_contains_photo_library_usage_description(self):
        plist_text = BACKGROUND_PLIST.read_text()
        self.assertIn("NSPhotoLibraryUsageDescription", plist_text)
        self.assertIn("photo library", plist_text)

    def test_photos_source_imports_respect_architecture_boundaries(self):
        domain_files = list((IOS_ROOT / "Domain" / "Photos").glob("*.swift"))
        self.assertTrue(len(domain_files) > 0)
        for df in domain_files:
            text = df.read_text()
            for forbidden in ["import SwiftUI", "import UIKit", "import Photos", "import SQLite3"]:
                self.assertNotIn(forbidden, text, f"Forbidden import {forbidden} in Domain file {df}")
