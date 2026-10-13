"""Portable P046 architecture checks; native behavioral evidence is XCTest on macOS."""
import re
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parents[2]


class BackgroundUploadRegistrationTests(unittest.TestCase):
    def test_new_sources_have_one_target_membership(self):
        project = (IOS / 'Synveil.xcodeproj/project.pbxproj').read_text()
        phases = re.findall(r'isa = PBXSourcesBuildPhase;.*?files = \((.*?)\);', project, re.S)
        for path in [
            'App/BackgroundUploadAppDelegate.swift',
            'Application/Transfer/BackgroundUploadCoordinator.swift',
            'Domain/Transfer/BackgroundUploadModels.swift',
            'Infrastructure/Network/URLSessionBackgroundUploadTransport.swift',
            'Infrastructure/Persistence/BackgroundUploadSQLiteSchema.swift',
            'Tests/SynveilTests/BackgroundUploadTests.swift',
        ]:
            self.assertTrue((IOS / path).is_file())
            ref = re.search(r'([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXFileReference;[^\n]*path = ' + re.escape(path) + ';', project)
            self.assertIsNotNone(ref, path)
            build = re.search(r'([A-F0-9]{24}) /\*.*?\*/ = \{isa = PBXBuildFile; fileRef = ' + ref.group(1), project)
            self.assertIsNotNone(build, path)
            target = int(path.startswith('Tests/'))
            self.assertIn(build.group(1), phases[target])
            self.assertNotIn(build.group(1), phases[1 - target])

    def test_background_transport_is_file_backed_and_rejects_redirects(self):
        source = (IOS / 'Infrastructure/Network/URLSessionBackgroundUploadTransport.swift').read_text()
        self.assertIn('URLSessionConfiguration.background(withIdentifier:', source)
        self.assertIn('uploadTask(with: native, fromFile: file)', source)
        self.assertNotIn('from: Data', source)
        self.assertIn('completionHandler(nil)', source)
        self.assertIn('session.allTasks', source)
        self.assertIn('task.taskDescription = intent.id', source)

    def test_background_owner_has_no_create_complete_abort_or_mutation_capability(self):
        source = (IOS / 'Application/Transfer/BackgroundUploadCoordinator.swift').read_text()
        for forbidden in ['repository.create(', 'repository.complete(', 'repository.abort(', '.submitMutation(', '.ack(', '.startRebaseline(']:
            self.assertNotIn(forbidden, source)
        self.assertIn('repository.status(', source)
        self.assertIn('database.resolveBackgroundJob(', source)
        self.assertNotIn('countOfBytesSent', source)

    def test_lifecycle_opt_in_and_schema_are_registered(self):
        app = (IOS / 'App/SynveilApp.swift').read_text()
        ui = (IOS / 'Features/Transfer/FileUploadControls.swift').read_text()
        store = (IOS / 'Infrastructure/Persistence/MutationQueueSQLiteStore.swift').read_text()
        self.assertIn('@UIApplicationDelegateAdaptor', app)
        self.assertIn('backgroundEnabled = false', ui)
        self.assertIn('synveil.upload.background.authorization', ui)
        self.assertIn('static let schemaVersion = 8', store)
        self.assertIn('try verifySchema(Self.version7Schema)', store)
        self.assertIn('backgroundBlocks(previous.id)', store)
        self.assertIn('encoder.outputFormatting = [.sortedKeys]', store)


if __name__ == '__main__':
    unittest.main()
