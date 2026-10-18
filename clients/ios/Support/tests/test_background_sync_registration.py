"""P048 portable configuration/architecture checks; behavior is verified by native XCTest."""
import plistlib
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class BackgroundSyncRegistrationTests(unittest.TestCase):
    def test_identifier_and_only_required_mode_match_both_app_configurations(self):
        info = plistlib.loads((IOS / 'App/BackgroundSyncInfo.plist').read_bytes())
        self.assertEqual(info['BGTaskSchedulerPermittedIdentifiers'], ['com.synveil.ios.sync.refresh.v1'])
        self.assertEqual(info['UIBackgroundModes'], ['fetch'])
        project = (IOS / 'Synveil.xcodeproj/project.pbxproj').read_text()
        self.assertEqual(project.count('INFOPLIST_FILE = App/BackgroundSyncInfo.plist;'), 2)
        self.assertEqual(project.count('PRODUCT_BUNDLE_IDENTIFIER = com.synveil.ios;'), 2)
        self.assertNotIn('BGProcessingTask', (IOS / 'App/BGAppRefreshScheduler.swift').read_text())

    def test_sources_have_unique_target_membership(self):
        project = (IOS / 'Synveil.xcodeproj/project.pbxproj').read_text()
        phases = re.findall(r'isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);', project, re.S)
        paths = [
            'Domain/Sync/BackgroundSyncModels.swift', 'Infrastructure/Persistence/BackgroundSyncPolicyStore.swift',
            'Application/Sync/BackgroundSyncCoordinator.swift', 'Application/Sync/BackgroundSyncWork.swift',
            'App/BGAppRefreshScheduler.swift', 'Infrastructure/Network/BackgroundSyncHTTPTransport.swift',
            'Features/Sync/BackgroundSyncSettingsView.swift',
            'Tests/SynveilTests/BackgroundSyncTests.swift', 'Tests/SynveilTests/BackgroundSyncSafetyTests.swift',
        ]
        for path in paths:
            self.assertTrue((IOS / path).is_file(), path)
            ref = re.search(r'([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXFileReference;[^\n]*path = ' + re.escape(path) + ';', project)
            self.assertIsNotNone(ref, path)
            build = re.search(r'([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXBuildFile; fileRef = ' + ref.group(1), project)
            target = int(path.startswith('Tests/'))
            self.assertEqual(phases[target].count(build.group(1)), 1)
            self.assertNotIn(build.group(1), phases[1 - target])

    def test_registration_precedes_existing_single_delegate_bootstrap(self):
        delegate = (IOS / 'App/BackgroundUploadAppDelegate.swift').read_text()
        self.assertLess(delegate.index('BGAppRefreshScheduler.shared.register()'), delegate.index('BackgroundUploadApplicationBootstrap.shared.start()'))
        app = (IOS / 'App/SynveilApp.swift').read_text()
        self.assertEqual(app.count('@UIApplicationDelegateAdaptor'), 1)
        self.assertIn('BackgroundUploadEventDelivery.shared.processPendingEvents()', app)
        self.assertIn('await dependencies.restoreBackgroundTransfers()', app)

    def test_background_layer_has_no_recovery_or_content_capabilities(self):
        source = (IOS / 'Application/Sync/BackgroundSyncCoordinator.swift').read_text()
        work = (IOS / 'Application/Sync/BackgroundSyncWork.swift').read_text()
        for forbidden in ['.recoverUnknownAcknowledgement(', '.drain(', '.startRebaseline(', '.createUpload(', '.patchUploadChunk(', '.completeUpload(', '.requestContent(']:
            self.assertNotIn(forbidden, source + work)
        self.assertIn('inbound.synchronize(scope: scope, configuration: .background)', work)
        self.assertIn('database.claimInboundRun(', work)
        self.assertIn('database.finishInboundRun(', work)
        self.assertNotIn('Timer', source)

    def test_opt_in_is_versioned_nonsecret_and_disabled_by_default(self):
        models = (IOS / 'Domain/Sync/BackgroundSyncModels.swift').read_text()
        storage = (IOS / 'Infrastructure/Persistence/BackgroundSyncPolicyStore.swift').read_text()
        self.assertIn('var enabled = false', models)
        self.assertIn('var version = 1', models)
        self.assertIn('policy.isValid', storage)
        self.assertIn('bytes.count <= 4096', storage)
        for forbidden in ['DeviceCredential', 'Bearer', 'ack_token', 'fileURL']:
            self.assertNotIn(forbidden, storage)

    def test_network_constraints_apply_to_existing_bounded_transport(self):
        source = (IOS / 'Infrastructure/Network/URLSessionHTTPTransport.swift').read_text()
        for setting in ['allowsCellularAccess', 'allowsExpensiveNetworkAccess', 'allowsConstrainedNetworkAccess']:
            self.assertIn('configuration.' + setting + ' = ' + setting, source)
        self.assertIn('configuration.waitsForConnectivity = false', source)
        guarded = (IOS / 'Infrastructure/Network/BackgroundSyncHTTPTransport.swift').read_text()
        self.assertIn('try gate.check()', guarded)
        self.assertIn('allowsConstrainedNetworkAccess: false', guarded)
        self.assertNotIn('NWPathMonitor', guarded)

    def test_ui_is_accessible_and_has_no_next_run_promise(self):
        source = (IOS / 'Features/Sync/BackgroundSyncSettingsView.swift').read_text()
        for identifier in ['enabled', 'network', 'status', 'confirmed', 'no-confirmation', 'recovery-required']:
            self.assertIn('synveil.background-sync.' + identifier, source)
        self.assertIn('iOS controls when', source)
        self.assertIn('Last saved server confirmation', source)
        self.assertNotIn('Next run', source)

    def test_v9_schema_and_durable_authorization_are_retained(self):
        source = (IOS / 'Infrastructure/Persistence/MutationQueueSQLiteStore.swift').read_text()
        self.assertIn('static let schemaVersion = 9', source)
        self.assertIn('row.1 == "AUTHORIZED", !row.2', source)
        self.assertIn('func requireBackgroundVerificationEligibility', source)
        body = source.split('func backgroundLibraryIdentifiers', 1)[1].split('\n    func ', 1)[0]
        for required in ['s.credential_id=?', 's.quarantined=0', 'ORDER BY s.library_id LIMIT ?', 'requireBackgroundVerificationEligibility']:
            self.assertIn(required, body)


if __name__ == '__main__':
    unittest.main()
