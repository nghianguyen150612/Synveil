"""Portable P047 registration and capability boundaries; native behavior uses XCTest."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class ClientRecoveryRegistrationTests(unittest.TestCase):
    def test_recovery_sources_have_exact_target_membership(self):
        project = (IOS / 'Synveil.xcodeproj/project.pbxproj').read_text()
        phases = re.findall(r'isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);', project, re.S)
        paths = [
            'Application/Session/SavedMetadataInspectionService.swift',
            'Application/Session/SessionAuthorizationEvidence.swift',
            'Infrastructure/Persistence/SessionAuthorizationSQLiteSchema.swift',
            'Tests/SynveilTests/SessionAuthorizationEvidenceTests.swift',
            'Domain/Recovery/ClientRecoveryPresentation.swift',
            'Features/Recovery/ClientRecoveryNotice.swift',
            'Features/Node/SavedMetadataInspectionView.swift',
            'Tests/SynveilTests/ClientRecoveryTests.swift',
            'Tests/SynveilTests/SavedMetadataInspectionTests.swift',
        ]
        for path in paths:
            self.assertTrue((IOS / path).is_file())
            ref = re.search(r'([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXFileReference;[^\n]*path = ' + re.escape(path) + ';', project)
            self.assertIsNotNone(ref, path)
            build = re.search(r'([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXBuildFile; fileRef = ' + ref.group(1), project)
            self.assertIsNotNone(build, path)
            target = int(path.startswith('Tests/'))
            self.assertEqual(phases[target].count(build.group(1)), 1)
            self.assertNotIn(build.group(1), phases[1 - target])

    def test_saved_inspection_reuses_saved_only_browser_without_write_authority(self):
        source = (IOS / 'Application/Session/SavedMetadataInspectionService.swift').read_text()
        self.assertIn('request: .savedOnly', source)
        self.assertIn('SQLiteNodeProjectionRepository(', source)
        self.assertIn('OfflineNodeBrowserService(', source)
        self.assertIn('try await provider.validate(session)', source)
        for forbidden in ['transport.send(', 'bindSession(', 'quarantineSessions(', '.synchronize(', '.acknowledge(', '.reconcileUnknown(', '.resume(', 'URLSession(']:
            self.assertNotIn(forbidden, source)
        store = (IOS / 'Infrastructure/Persistence/MutationQueueSQLiteStore.swift').read_text()
        read = store.split('func savedLibraries(')[1].split('func cachedProjectionState(')[0]
        self.assertIn('s.credential_id=? AND s.quarantined=0', read)
        self.assertIn('JOIN sync_bases', read)
        self.assertIn('JOIN cached_libraries', read)
        self.assertNotIn('ensureScope(', read)
        self.assertNotIn('UPDATE ', read)
        self.assertNotIn('INSERT ', read)

    def test_presentation_has_no_retry_executor_or_connectivity_mutation(self):
        for path in ['Domain/Recovery/ClientRecoveryPresentation.swift', 'Features/Recovery/ClientRecoveryNotice.swift']:
            source = (IOS / path).read_text()
            for forbidden in ['URLSession', 'NWPathMonitor', 'Timer', 'transport.send', 'MutationDrainCoordinator(', 'SyncAckService(', '.synchronize(']:
                self.assertNotIn(forbidden, source)
        model = (IOS / 'Features/Transfer/FileUploadViewModel.swift').read_text()
        self.assertNotIn('try?', model)
        self.assertIn('backgroundJobs[record.id]?.blocksForeground != true', model)
        self.assertIn('generation == statusGeneration', model)

    def test_unknown_network_errors_fail_closed(self):
        source = (IOS / 'Domain/Services/Transport/SynveilTransportError.swift').read_text()
        self.assertIn('default: return .malformedResponse', source)
        self.assertNotIn('localizedDescription', source)
        transport = (IOS / 'Infrastructure/Network/URLSessionHTTPTransport.swift').read_text()
        self.assertIn('defer { bytesAsync.task.cancel() }', transport)
        self.assertIn('throw SynveilTransportError.classify(error)', transport)

    def test_durable_authorization_marker_precedes_verification(self):
        service = (IOS / 'Application/Session/SessionRestorationService.swift').read_text()
        verify = service.split('private func verify(')[1].split('private func confirmSessionIdentity')[0]
        self.assertLess(verify.index('authorizationEvidence?.begin(session)'), verify.index('authorizationValidator.validateAuthorization'))
        composition = (IOS / 'App/AppDependencyContainer.swift').read_text()
        self.assertIn('authorizationEvidence: SQLiteSessionAuthorizationEvidence(database: nil)', composition)
        self.assertIn('service = guardedRestoration', composition)
        inspection = (IOS / 'Application/Session/SavedMetadataInspectionService.swift').read_text()
        self.assertIn('permitsPendingSavedInspection', inspection)

    def test_authorization_schema_rejects_forgery_and_identity_rewrite(self):
        import sqlite3
        source = (IOS / 'Infrastructure/Persistence/SessionAuthorizationSQLiteSchema.swift').read_text()
        statements = re.findall(r'"""(.*?)"""', source, re.S)
        authority = [0]
        with sqlite3.connect(':memory:') as db:
            db.create_function('synveil_projection_authorized', 0, lambda: authority[0])
            for sql in statements:
                db.execute(sql)
            values = ['https://example.test'] + ['a' * 36] * 4 + ['CHECKING', 0]
            with self.assertRaises(sqlite3.DatabaseError):
                db.execute('INSERT INTO session_authorization_checks VALUES(?,?,?,?,?,?,?)', values)
            authority[0] = 9
            db.execute('INSERT INTO session_authorization_checks VALUES(?,?,?,?,?,?,?)', values)
            with self.assertRaises(sqlite3.DatabaseError):
                db.execute("UPDATE session_authorization_checks SET owner_id=?", ['b' * 36])
            with self.assertRaises(sqlite3.DatabaseError):
                db.execute('DELETE FROM session_authorization_checks')
            authority[0] = 0
            with self.assertRaises(sqlite3.DatabaseError):
                db.execute("UPDATE session_authorization_checks SET state='TRANSIENT'")
            self.assertEqual(db.execute('SELECT state FROM session_authorization_checks').fetchone(), ('CHECKING',))
