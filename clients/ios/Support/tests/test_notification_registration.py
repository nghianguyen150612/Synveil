"""P049 target membership and capability-boundary regression guards."""

import re
import unittest
from pathlib import Path


IOS = Path(__file__).resolve().parents[2]


class NotificationRegistrationTests(unittest.TestCase):
    def test_production_sources_are_registered(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        sources = list((IOS / "Application/Notifications").glob("*.swift"))
        sources += list((IOS / "Domain/Notifications").glob("*.swift"))
        sources += list((IOS / "Features/Notifications").glob("*.swift"))
        sources += list((IOS / "Infrastructure/Notifications").glob("*.swift"))
        sources += list((IOS / "Infrastructure/Persistence").glob("Notification*.swift"))
        self.assertEqual(len(sources), 9)
        app_sources = project.split("A80000012C00000000000001 /* Sources */ = {")[1].split("};")[0]
        for source in sources:
            self.assertIn(source.relative_to(IOS).as_posix(), project)
            self.assertIn(source.name + " in Sources", app_sources)

    def test_test_sources_are_registered(self):
        project = (IOS / "Synveil.xcodeproj/project.pbxproj").read_text()
        target = project.split("B80000012C00000000000001 /* Sources */ = {")[1].split("};")[0]
        sources = list((IOS / "Tests/SynveilTests").glob("Notification*.swift"))
        self.assertEqual(len(sources), 6)
        for source in sources:
            self.assertIn(source.name + " in Sources", target)

    def test_one_application_delegate_owns_native_notification_callbacks(self):
        app = (IOS / "App/SynveilApp.swift").read_text()
        delegate = (IOS / "App/BackgroundUploadAppDelegate.swift").read_text()
        self.assertEqual(app.count("@UIApplicationDelegateAdaptor"), 1)
        self.assertEqual(delegate.count("UNUserNotificationCenter.current().delegate = self"), 1)
        self.assertIn("UNUserNotificationCenterDelegate", delegate)
        self.assertIn("handleEventsForBackgroundURLSession", delegate)
        self.assertIn("BGAppRefreshScheduler.shared.register()", delegate)

    def test_notification_application_layer_has_no_operation_capabilities(self):
        forbidden = r"\.(?:completeUpload|patchUploadChunk|createUploadSession|recoverUnknownAcknowledgement|synchronize|prepareUpload|updateUpload|updateDownload|rebaseline|requestContent)\s*\("
        for source in (IOS / "Application/Notifications").glob("*.swift"):
            self.assertIsNone(re.search(forbidden, source.read_text()), source.name)

    def test_notification_adapter_does_not_register_remote_push(self):
        sources = list((IOS / "Infrastructure/Notifications").glob("*.swift"))
        sources += [IOS / "App/BackgroundUploadAppDelegate.swift"]
        for source in sources:
            self.assertNotIn("registerForRemoteNotifications", source.read_text())
            self.assertNotIn("didRegisterForRemoteNotifications", source.read_text())

    def test_sqlite_remains_v9_and_notification_failure_is_isolated(self):
        database = (IOS / "Infrastructure/Persistence/MutationQueueSQLiteStore.swift").read_text()
        self.assertIn("static let schemaVersion = 9", database)
        self.assertNotIn("Notification", database)
        container = (IOS / "App/AppDependencyContainer.swift").read_text()
        self.assertIn("catch { notifications = nil }", container)


if __name__ == "__main__":
    unittest.main()
