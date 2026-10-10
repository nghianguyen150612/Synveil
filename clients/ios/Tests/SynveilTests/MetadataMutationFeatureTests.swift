import Foundation
import XCTest

@testable import Synveil

@MainActor
final class MetadataMutationFeatureTests: XCTestCase {
    func testCreateFolderUsesLibraryRootAndAuthoritativeRootRevisionWithoutPosting() async throws {
        let fixture = try await queueFixture(self)
        let rootId = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let root = try await mutationNode(
            81, scope: fixture.scope, parent: nil, revision: "27", kind: .directory)
        let repository = MutationNodeRepository(nodes: [root])
        let generator = CountingMutationIDGenerator(first: 700)
        let feature = makeFeature(fixture, repository: repository, generator: generator)
        let library = mutationLibrary(fixture, root: rootId)
        let networkCount = await fixture.transport.requests().count
        await fixture.transport.fail(.offline)

        let result = await feature.enqueue(
            .createFolder(
                parentNodeId: rootId, parentAncestry: [rootId], parentSnapshot: nil, name: "資料 🚀"),
            in: library)

        guard case .persisted(let receipt) = result else {
            return XCTFail("Expected durable enqueue")
        }
        XCTAssertEqual(receipt.kind, .createDirectory)
        XCTAssertEqual(receipt.state, .pending)
        let generated = await generator.generatedCount()
        let requestsAfter = await fixture.transport.requests()
        XCTAssertEqual(generated, 1)
        XCTAssertEqual(requestsAfter.count, networkCount)
        let id = try await ClientMutationId.validated(receipt.mutationId, using: QueueValidator())
        guard
            case .record(let record) = await fixture.queue.get(scope: fixture.scope, mutationId: id),
            case .createDirectory(let parentId, let revision, let name) = record.mutation.payload
                .intent
        else { return XCTFail("Expected stored CREATE_DIRECTORY intent") }
        XCTAssertEqual(parentId, rootId)
        XCTAssertEqual(revision.rawValue, "27")
        XCTAssertEqual(name, "資料 🚀")
        XCTAssertEqual(record.state, .pending)
    }

    func testMissingRootRevisionFailsClosedWithoutGeneratingAnID() async throws {
        let fixture = try await queueFixture(self)
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let repository = MutationNodeRepository(nodes: [])
        let generator = CountingMutationIDGenerator(first: 700)
        let feature = makeFeature(fixture, repository: repository, generator: generator)

        let result = await feature.enqueue(
            .createFolder(
                parentNodeId: root, parentAncestry: [root], parentSnapshot: nil, name: "New"),
            in: mutationLibrary(fixture, root: root))

        XCTAssertEqual(result, .failed(.invalidMetadata))
        let generated = await generator.generatedCount()
        XCTAssertEqual(generated, 0)
        XCTAssertEqual(try queueRawScalar(fixture.url, "SELECT count(*) FROM mutations"), "0")
    }

    func testMissingVerifiedBaseOffersExplicitSetupAndBlocksEnqueue() async throws {
        let fixture = try await queueFixture(self, initializeBase: false)
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let node = try await mutationNode(81, scope: fixture.scope, parent: nil, kind: .directory)
        let repository = MutationNodeRepository(nodes: [node])
        let generator = CountingMutationIDGenerator(first: 700)
        let feature = makeFeature(fixture, repository: repository, generator: generator)
        let library = mutationLibrary(fixture, root: root)
        let networkCount = await fixture.transport.requests().count

        let availability = await feature.availability(in: library)
        let requestsBeforeSetup = await fixture.transport.requests()
        XCTAssertEqual(availability, .setupRequired)
        XCTAssertEqual(requestsBeforeSetup.count, networkCount)
        let blocked = await feature.enqueue(
            .createFolder(
                parentNodeId: root, parentAncestry: [root], parentSnapshot: nil, name: "New"),
            in: library)
        XCTAssertEqual(blocked, .failed(.syncBaseRequired))
        let generatedBeforeSetup = await generator.generatedCount()
        XCTAssertEqual(generatedBeforeSetup, 0)

        let setup = await feature.enableChanges(in: library)
        let generatedAfterSetup = await generator.generatedCount()
        XCTAssertEqual(setup, .prepared)
        XCTAssertEqual(generatedAfterSetup, 0)
        let requests = await fixture.transport.requests()
        XCTAssertEqual(requests.count, networkCount + 1)
        XCTAssertEqual(requests.last?.method, .get)
        XCTAssertFalse(requests.contains(where: { $0.method == .post }))
    }

    func testOfflineCheckpointSetupHasTypedErrorAndDoesNotSendMutation() async throws {
        let fixture = try await queueFixture(self, initializeBase: false)
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let repository = MutationNodeRepository(nodes: [
            try await mutationNode(81, scope: fixture.scope, parent: nil, kind: .directory)
        ])
        let feature = makeFeature(
            fixture, repository: repository, generator: CountingMutationIDGenerator(first: 700))
        await fixture.transport.fail(.offline)

        let result = await feature.enableChanges(in: mutationLibrary(fixture, root: root))

        XCTAssertEqual(result, .failed(.offline))
        let requests = await fixture.transport.requests()
        XCTAssertEqual(requests.map(\.method), [.get])
    }

    func testReadOnlyAndQuarantinedLibrariesCannotEnqueue() async throws {
        let fixture = try await queueFixture(self)
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let node = try await mutationNode(81, scope: fixture.scope, parent: nil, kind: .directory)
        let repository = MutationNodeRepository(nodes: [node])
        let generator = CountingMutationIDGenerator(first: 700)
        let feature = makeFeature(fixture, repository: repository, generator: generator)

        let readOnly = mutationLibrary(fixture, root: root, status: .readOnly)
        let readOnlyAvailability = await feature.availability(in: readOnly)
        XCTAssertEqual(readOnlyAvailability, .unavailable(.libraryReadOnly))
        let readOnlyEnqueue = await feature.enqueue(
            .createFolder(
                parentNodeId: root, parentAncestry: [root], parentSnapshot: nil, name: "x"),
            in: readOnly)
        XCTAssertEqual(readOnlyEnqueue, .failed(.readOnlyLibrary))

        let quarantined = mutationLibrary(fixture, root: root, status: .quarantined)
        let quarantineAvailability = await feature.availability(in: quarantined)
        XCTAssertEqual(quarantineAvailability, .unavailable(.libraryQuarantined))
        let quarantineEnqueue = await feature.enqueue(
            .createFolder(
                parentNodeId: root, parentAncestry: [root], parentSnapshot: nil, name: "x"),
            in: quarantined)
        XCTAssertEqual(quarantineEnqueue, .failed(.quarantinedLibrary))
        let generated = await generator.generatedCount()
        XCTAssertEqual(generated, 0)
        XCTAssertEqual(try queueRawScalar(fixture.url, "SELECT count(*) FROM mutations"), "0")
    }

    func testInvalidNameDoesNotCreateAnOperationAndUnicodeRenameIsPreserved() async throws {
        let fixture = try await queueFixture(self)
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let source = try await mutationNode(82, scope: fixture.scope, parent: root, revision: "19")
        let repository = MutationNodeRepository(nodes: [source])
        let generator = CountingMutationIDGenerator(first: 700)
        let feature = makeFeature(fixture, repository: repository, generator: generator)
        let library = mutationLibrary(fixture, root: root)

        let invalidName = await feature.enqueue(.rename(node: source, newName: ""), in: library)
        XCTAssertEqual(invalidName, .failed(.invalidName))
        let generatedBeforeValid = await generator.generatedCount()
        XCTAssertEqual(generatedBeforeValid, 0)
        let result = await feature.enqueue(.rename(node: source, newName: "résumé 🧭"), in: library)
        guard case .persisted(let receipt) = result else {
            return XCTFail("Expected rename enqueue")
        }
        let id = try await ClientMutationId.validated(receipt.mutationId, using: QueueValidator())
        guard
            case .record(let record) = await fixture.queue.get(scope: fixture.scope, mutationId: id),
            case .renameNode(let nodeId, let revision, let name) = record.mutation.payload.intent
        else { return XCTFail("Expected stored RENAME_NODE intent") }
        XCTAssertEqual(nodeId, source.id)
        XCTAssertEqual(revision.rawValue, "19")
        XCTAssertEqual(name, "résumé 🧭")
        let generatedAfterValid = await generator.generatedCount()
        XCTAssertEqual(generatedAfterValid, 1)
    }

    func testDuplicateSaveTapSharesOneIdentityAndOneSQLiteRecord() async throws {
        let fixture = try await queueFixture(self)
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let node = try await mutationNode(81, scope: fixture.scope, parent: nil, kind: .directory)
        let gate = QueueGate()
        let repository = MutationNodeRepository(nodes: [node], metadataGate: gate)
        let generator = CountingMutationIDGenerator(first: 700)
        let feature = makeFeature(fixture, repository: repository, generator: generator)
        let library = mutationLibrary(fixture, root: root)
        let command = MetadataMutationCommand.createFolder(
            parentNodeId: root, parentAncestry: [root], parentSnapshot: nil, name: "Single save")

        let first = Task { await feature.enqueue(command, in: library) }
        await gate.wait()
        let duplicate = await feature.enqueue(command, in: library)
        XCTAssertEqual(duplicate, .failed(.duplicateSubmission))
        await gate.release()
        guard case .persisted = await first.value else {
            return XCTFail("First save should commit")
        }
        let generated = await generator.generatedCount()
        XCTAssertEqual(generated, 1)
        XCTAssertEqual(try queueRawScalar(fixture.url, "SELECT count(*) FROM mutations"), "1")
    }

    func testLostCommitAcknowledgementReadsBackTheOriginalIntentIdentity() async throws {
        let fixture = try await queueFixture(self)
        let storage = QueueDelayedStorage(
            base: fixture.database, gate: QueueGate(), delayResult: true,
            loseEnqueueAcknowledgement: true)
        let queue = DurableMutationQueue(
            store: storage, provider: fixture.provider, bridge: QueueValidator())
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let rootNode = try await mutationNode(
            81, scope: fixture.scope, parent: nil, kind: .directory)
        let feature = makeFeature(
            fixture, repository: MutationNodeRepository(nodes: [rootNode]),
            generator: CountingMutationIDGenerator(first: 700), queue: queue)
        let library = mutationLibrary(fixture, root: root)
        _ = await feature.availability(in: library)
        let command = MetadataMutationCommand.createFolder(
            parentNodeId: root, parentAncestry: [root], parentSnapshot: rootNode,
            name: "One logical save")

        let first = await feature.enqueue(command, in: library)
        XCTAssertEqual(first, .failed(.commitAcknowledgementUncertain))
        let retry = await feature.enqueue(command, in: library)

        guard case .persisted(let receipt) = retry else {
            return XCTFail("Readback should return the committed original operation")
        }
        XCTAssertEqual(receipt.mutationId, queueUUID(700))
        XCTAssertEqual(try queueRawScalar(fixture.url, "SELECT count(*) FROM mutations"), "1")
        let id = try await ClientMutationId.validated(receipt.mutationId, using: QueueValidator())
        guard
            case .record(let record) = await fixture.queue.get(scope: fixture.scope, mutationId: id)
        else { return XCTFail("Expected the original committed queue row") }
        XCTAssertEqual(record.state, .pending)
        XCTAssertEqual(record.mutation.payload.intent.kind, .createDirectory)
    }

    func testMoveRejectsSelfCrossLibraryAndKnownDescendant() async throws {
        let fixture = try await queueFixture(self)
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let source = try await mutationNode(
            82, scope: fixture.scope, parent: root, kind: .directory)
        let child = try await mutationNode(
            83, scope: fixture.scope, parent: source.id, kind: .directory)
        let rootNode = try await mutationNode(
            81, scope: fixture.scope, parent: nil, kind: .directory)
        let fileDestination = try await mutationNode(85, scope: fixture.scope, parent: root)
        let repository = MutationNodeRepository(nodes: [rootNode, source, child, fileDestination])
        let generator = CountingMutationIDGenerator(first: 700)
        let feature = makeFeature(fixture, repository: repository, generator: generator)
        let library = mutationLibrary(fixture, root: root)

        let selfMove = await feature.enqueue(
            .move(node: source, destination: source, destinationAncestry: [root, source.id]),
            in: library)
        XCTAssertEqual(selfMove, .failed(.invalidMetadata))
        let descendantMove = await feature.enqueue(
            .move(
                node: source, destination: child,
                destinationAncestry: [root, source.id, child.id]),
            in: library)
        XCTAssertEqual(descendantMove, .failed(.invalidMetadata))
        let sameParentMove = await feature.enqueue(
            .move(node: source, destination: rootNode, destinationAncestry: [root]),
            in: library)
        XCTAssertEqual(sameParentMove, .failed(.invalidMetadata))
        let fileMove = await feature.enqueue(
            .move(node: source, destination: fileDestination, destinationAncestry: [root]),
            in: library)
        XCTAssertEqual(fileMove, .failed(.invalidMetadata))
        let otherLibrary = try await LibraryId.validated(queueUUID(99), using: QueueValidator())
        let foreign = try await mutationNode(
            84, scope: fixture.scope, libraryId: otherLibrary, parent: root, kind: .directory)
        let crossLibraryMove = await feature.enqueue(
            .move(node: source, destination: foreign, destinationAncestry: [root, foreign.id]),
            in: library)
        XCTAssertEqual(crossLibraryMove, .failed(.invalidMetadata))
        let generated = await generator.generatedCount()
        XCTAssertEqual(generated, 0)
        XCTAssertEqual(try queueRawScalar(fixture.url, "SELECT count(*) FROM mutations"), "0")
    }

    func testMovePersistsExactSourceAndDestinationRevisions() async throws {
        let fixture = try await queueFixture(self)
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let source = try await mutationNode(82, scope: fixture.scope, parent: root, revision: "31")
        let destination = try await mutationNode(
            83, scope: fixture.scope, parent: root, revision: "84", kind: .directory)
        let feature = makeFeature(
            fixture, repository: MutationNodeRepository(nodes: [source, destination]),
            generator: CountingMutationIDGenerator(first: 700))
        let library = mutationLibrary(fixture, root: root)

        let result = await feature.enqueue(
            .move(
                node: source, destination: destination, destinationAncestry: [root, destination.id]),
            in: library)

        guard case .persisted(let receipt) = result else {
            return XCTFail("Expected MOVE_NODE to queue")
        }
        let mutationId = try await ClientMutationId.validated(
            receipt.mutationId, using: QueueValidator())
        guard
            case .record(let record) = await fixture.queue.get(
                scope: fixture.scope, mutationId: mutationId),
            case .moveNode(let nodeId, let sourceRevision, let parentId, let parentRevision) =
                record.mutation.payload.intent
        else { return XCTFail("Expected durable MOVE_NODE with both exact revisions") }
        XCTAssertEqual(nodeId, source.id)
        XCTAssertEqual(sourceRevision.rawValue, "31")
        XCTAssertEqual(parentId, destination.id)
        XCTAssertEqual(parentRevision.rawValue, "84")
    }

    func testTrashRequiresExplicitConfirmation() async throws {
        let fixture = try await queueFixture(self)
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let source = try await mutationNode(82, scope: fixture.scope, parent: root)
        let generator = CountingMutationIDGenerator(first: 700)
        let feature = makeFeature(
            fixture, repository: MutationNodeRepository(nodes: [source]), generator: generator)

        let result = await feature.enqueue(
            .trash(node: source, confirmed: false), in: mutationLibrary(fixture, root: root))
        XCTAssertEqual(result, .failed(.confirmationRequired))
        let generated = await generator.generatedCount()
        XCTAssertEqual(generated, 0)
        XCTAssertEqual(try queueRawScalar(fixture.url, "SELECT count(*) FROM mutations"), "0")
    }

    func testAppliedTrashRestoreUsesCurrentParentRevisionAndNeverGuesses() async throws {
        let fixture = try await queueFixture(self)
        let rootId = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let root = try await mutationNode(
            81, scope: fixture.scope, parent: nil, revision: "44", kind: .directory)
        let source = try await mutationNode(82, scope: fixture.scope, parent: rootId, revision: "5")
        let repository = MutationNodeRepository(nodes: [root, source])
        let generator = CountingMutationIDGenerator(first: 700)
        let feature = makeFeature(fixture, repository: repository, generator: generator)
        let library = mutationLibrary(fixture, root: rootId)
        let initialParentAvailability = await feature.folderParentAvailability(
            in: library, parentNodeId: rootId, parentSnapshot: root, forceRefresh: false)
        XCTAssertEqual(initialParentAvailability, .ready)
        guard
            case .persisted(let trashReceipt) = await feature.enqueue(
                .trash(node: source, confirmed: true), in: library)
        else { return XCTFail("Expected trash operation to queue") }
        let trashId = try await ClientMutationId.validated(
            trashReceipt.mutationId, using: QueueValidator())
        guard
            case .record(let trashRecord) = await fixture.queue.get(
                scope: fixture.scope, mutationId: trashId)
        else { return XCTFail("Missing durable trash record") }
        await fixture.transport.set(try appliedTrashResponse(trashRecord.mutation, source: source))

        let drain = await feature.sendPendingChanges(in: library)
        XCTAssertEqual(drain.applied, 1)
        XCTAssertNil(drain.stop)
        let refreshedRoot = try await mutationNode(
            81, scope: fixture.scope, parent: nil, revision: "45", kind: .directory)
        repository.replace(refreshedRoot)
        let currentParentAvailability = await feature.folderParentAvailability(
            in: library, parentNodeId: rootId, parentSnapshot: root, forceRefresh: false)
        XCTAssertEqual(currentParentAvailability, .ready)
        XCTAssertEqual(repository.metadataRequestCount, 1)
        guard case .loaded(let activity) = await feature.activity(in: library, limit: 10),
            let appliedTrash = activity.first(where: { $0.id == trashReceipt.mutationId })
        else { return XCTFail("Expected applied trash history") }
        XCTAssertEqual(appliedTrash.state, .applied)
        XCTAssertTrue(appliedTrash.mayCheckRestore)
        let restoreCheck = await feature.checkRestore(
            trashedOperationId: trashReceipt.mutationId, in: library)
        XCTAssertNil(restoreCheck)

        guard
            case .persisted(let restoreReceipt) = await feature.enqueueRestore(
                trashedOperationId: trashReceipt.mutationId, in: library)
        else { return XCTFail("Expected guarded restore enqueue") }
        let restoreId = try await ClientMutationId.validated(
            restoreReceipt.mutationId, using: QueueValidator())
        guard
            case .record(let restoreRecord) = await fixture.queue.get(
                scope: fixture.scope, mutationId: restoreId),
            case .restoreNode(let nodeId, let nodeRevision, let parentId, let parentRevision) =
                restoreRecord.mutation.payload.intent
        else { return XCTFail("Expected RESTORE_NODE with authoritative preconditions") }
        XCTAssertEqual(nodeId, source.id)
        XCTAssertEqual(nodeRevision.rawValue, "6")
        XCTAssertEqual(parentId, rootId)
        XCTAssertEqual(parentRevision.rawValue, "45")
        XCTAssertEqual(restoreRecord.state, .pending)

        guard
            case .persisted(let folderReceipt) = await feature.enqueue(
                .createFolder(
                    parentNodeId: rootId, parentAncestry: [rootId], parentSnapshot: root,
                    name: "After restore"),
                in: library)
        else { return XCTFail("Expected a new folder to use the refreshed parent revision") }
        let folderId = try await ClientMutationId.validated(
            folderReceipt.mutationId, using: QueueValidator())
        guard
            case .record(let folderRecord) = await fixture.queue.get(
                scope: fixture.scope, mutationId: folderId),
            case .createDirectory(_, let folderParentRevision, _) =
                folderRecord.mutation.payload.intent
        else { return XCTFail("Expected durable CREATE_DIRECTORY") }
        XCTAssertEqual(folderParentRevision.rawValue, "45")
    }

    func testMissingFolderParentRevisionBlocksOnlyFolderCreation() async throws {
        let fixture = try await queueFixture(self)
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let library = mutationLibrary(fixture, root: root)
        let feature = makeFeature(
            fixture, repository: MutationNodeRepository(nodes: []),
            generator: CountingMutationIDGenerator(first: 700))
        let controller = try authenticatedMutationController()
        let route = NodeBrowserRoute(
            library: NodeBrowserLibraryContext(
                id: library.id, name: library.name, rootNodeId: root, status: library.status),
            parentScope: .libraryRoot(rootNodeId: root), directoryTitle: library.name,
            ancestry: [root])
        let viewModel = MetadataMutationViewModel(
            route: route, feature: feature, sessionController: controller)

        await viewModel.refreshAvailability()

        XCTAssertEqual(viewModel.availability, .ready)
        XCTAssertEqual(viewModel.folderParentAvailability, .unavailable(.invalidMetadata))
        XCTAssertTrue(viewModel.canEdit)
        XCTAssertFalse(viewModel.canCreateFolder)
    }

    func testRestoreIsUnavailableForPendingTrashOrUnverifiedCurrentParent() async throws {
        let fixture = try await queueFixture(self)
        let rootId = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        let source = try await mutationNode(82, scope: fixture.scope, parent: rootId, revision: "5")
        let generator = CountingMutationIDGenerator(first: 700)
        let feature = makeFeature(
            fixture, repository: MutationNodeRepository(nodes: [source]), generator: generator)
        let library = mutationLibrary(fixture, root: rootId)
        guard
            case .persisted(let trashReceipt) = await feature.enqueue(
                .trash(node: source, confirmed: true), in: library)
        else { return XCTFail("Expected trash operation to queue") }

        let pendingRestore = await feature.checkRestore(
            trashedOperationId: trashReceipt.mutationId, in: library)
        XCTAssertEqual(pendingRestore, .operationUnavailable)

        let trashId = try await ClientMutationId.validated(
            trashReceipt.mutationId, using: QueueValidator())
        guard
            case .record(let trashRecord) = await fixture.queue.get(
                scope: fixture.scope, mutationId: trashId)
        else { return XCTFail("Missing durable trash record") }
        await fixture.transport.set(try appliedTrashResponse(trashRecord.mutation, source: source))
        let drain = await feature.sendPendingChanges(in: library)
        XCTAssertEqual(drain.applied, 1)
        let missingParentRestore = await feature.checkRestore(
            trashedOperationId: trashReceipt.mutationId, in: library)
        XCTAssertEqual(missingParentRestore, .invalidMetadata)
        let generated = await generator.generatedCount()
        XCTAssertEqual(generated, 1)
        XCTAssertEqual(try queueRawScalar(fixture.url, "SELECT count(*) FROM mutations"), "1")
    }

    func testQueueActivityReadIsBoundedAndTerminalHistoryDoesNotStarvePending() async throws {
        let fixture = try await queueFixture(self)
        for offset in 0..<18 {
            let mutation = try await queuePrepared(
                id: 100 + offset, node: 300 + offset, scope: fixture.scope)
            let row = try await queueEnqueued(fixture, mutation: mutation)
            let lease = try await fixture.queue.acquireAttempt(
                scope: fixture.scope, mutationId: row.mutation.id)
            try await fixture.queue.finish(
                lease, result: .failed(.permanentRejection(.notFound)))
        }
        let pending = try await queuePrepared(id: 500, node: 700, scope: fixture.scope)
        let pendingRow = try await queueEnqueued(fixture, mutation: pending)

        guard case .records(let rows) = await fixture.queue.activity(scope: fixture.scope, limit: 5)
        else { return XCTFail("Expected bounded activity rows") }
        XCTAssertEqual(rows.count, 5)
        XCTAssertEqual(rows.map(\.enqueueOrder), rows.map(\.enqueueOrder).sorted())
        XCTAssertTrue(rows.contains(where: { $0.mutation.id == pendingRow.mutation.id }))
        XCTAssertEqual(rows.last?.state, .pending)
    }

    func testQueueActivityRejectsDifferentDeviceScope() async throws {
        let fixture = try await queueFixture(self)
        _ = try await queueEnqueued(fixture)
        let other = try await queueScope(device: 93)
        guard case .failed(let failure) = await fixture.queue.activity(scope: other, limit: 10)
        else { return XCTFail("Different Device must not receive queue rows") }
        XCTAssertEqual(failure, .scopeMismatch)
    }

    func testDifferentLibraryCannotSeeOtherLibraryActivity() async throws {
        let fixture = try await queueFixture(self)
        let saved = try await queueEnqueued(fixture)
        let otherLibraryScope = try await queueScope(device: 91, library: 94)

        guard
            case .records(let rows) = await fixture.queue.activity(
                scope: otherLibraryScope, limit: 10)
        else { return XCTFail("Expected scoped empty activity") }
        XCTAssertTrue(rows.isEmpty)
        XCTAssertFalse(rows.contains(where: { $0.mutation.id == saved.mutation.id }))
    }

    func testActivityTargetAndDiagnosticsNeverExposeCredentialsOrRequestBody() async throws {
        let fixture = try await queueFixture(self)
        let mutation = try await queuePrepared(name: "Unicode 資料", scope: fixture.scope)
        let row = try await queueEnqueued(fixture, mutation: mutation)
        let feature = makeFeature(
            fixture, repository: MutationNodeRepository(nodes: []),
            generator: CountingMutationIDGenerator(first: 700))

        guard
            case .loaded(let items) = await feature.activity(
                in: mutationLibrary(
                    fixture,
                    root: try await NodeId.validated(queueUUID(81), using: QueueValidator())),
                limit: 10),
            let item = items.first(where: { $0.id == row.mutation.id.rawValue })
        else { return XCTFail("Expected durable activity item") }
        let description = String(describing: item)
        XCTAssertFalse(description.contains(queueBearer))
        XCTAssertFalse(item.targetLabel.contains(queueBearer))
        XCTAssertFalse(item.targetLabel.contains("requestBody"))
        XCTAssertEqual(item.state, .pending)
    }

    func testEveryDurableStateHasDistinctUserFacingLabelAndSymbol() {
        let expected: [(MutationQueueState, String)] = [
            (.pending, "Queued"), (.submitting, "Sending"), (.applied, "Applied"),
            (.conflict, "Conflict — review needed"), (.outcomeUnknown, "Outcome unknown"),
            (.blockedRebaseline, "Sync recovery required"), (.failedPermanent, "Could not apply"),
        ]
        for (state, label) in expected {
            XCTAssertEqual(MutationQueueStatePresentation.label(for: state), label)
            XCTAssertFalse(MutationQueueStatePresentation.symbol(for: state).isEmpty)
        }
        XCTAssertNotEqual(
            MutationQueueStatePresentation.label(for: .conflict),
            MutationQueueStatePresentation.label(for: .outcomeUnknown))
        XCTAssertNotEqual(
            MutationQueueStatePresentation.label(for: .failedPermanent),
            MutationQueueStatePresentation.label(for: .pending))
    }

    func testConflictReasonsUseOnlyDocumentedAllowlistAndNoResolutionButtons() {
        XCTAssertEqual(
            Set(ClientMutationConflictReason.allCases.map(\.rawValue)),
            [
                "REVISION_MISMATCH", "NODE_STATE_CHANGED", "PARENT_CHANGED", "NAME_OCCUPIED",
                "DESTINATION_CHANGED", "RESOURCE_PURGED",
            ])
        for reason in ClientMutationConflictReason.allCases {
            XCTAssertTrue(
                MutationConflictPresentation.message(for: reason).contains(reason.rawValue))
        }
        XCTAssertFalse(
            MutationConflictPresentation.message(for: .revisionMismatch)
                .localizedCaseInsensitiveContains("accept server"))
        XCTAssertFalse(
            MutationConflictPresentation.message(for: .revisionMismatch)
                .localizedCaseInsensitiveContains("overwrite"))
    }

    func testActivityScreenAppearanceDoesNotDrainAndExplicitSendCallsFeatureOnce() async throws {
        let controller = try authenticatedMutationController()
        let library = try await presentationLibrary(status: .active)
        let feature = FakeMetadataMutationFeature()
        let viewModel = MutationActivityViewModel(
            library: library, feature: feature, sessionController: controller)

        await viewModel.load()
        XCTAssertEqual(feature.sendCount, 0)
        await viewModel.sendPendingChanges()
        XCTAssertEqual(feature.sendCount, 1)
        XCTAssertEqual(
            viewModel.notice?.message,
            "1 change applied. Review Pending Changes for any remaining operations.")
    }

    func testDrainSummaryUsesCoordinatorOutcomesAndNoopSendDoesNotClaimQueuedWork() async throws {
        let controller = try authenticatedMutationController()
        let library = try await presentationLibrary(status: .active)
        let feature = FakeMetadataMutationFeature()
        let viewModel = MutationActivityViewModel(
            library: library, feature: feature, sessionController: controller)
        feature.drainResult = MetadataMutationDrainPresentation(
            attempted: 1, applied: 0, conflicts: 0, unknown: 1,
            permanentRejections: 0, rebaselineBlocked: 0, localFailures: 0,
            stop: .unknownNeedsReconciliation)
        await viewModel.load()
        await viewModel.sendPendingChanges()
        XCTAssertTrue(viewModel.notice?.message.contains("1 change has an unknown outcome") == true)
        XCTAssertEqual(viewModel.notice?.title, "Pending Changes updated")

        feature.drainResult = MetadataMutationDrainPresentation(
            attempted: 0, applied: 0, conflicts: 0, unknown: 0,
            permanentRejections: 0, rebaselineBlocked: 0, localFailures: 0, stop: nil)
        await viewModel.sendPendingChanges()
        XCTAssertTrue(
            viewModel.notice?.message.contains("No queued changes were available") == true)
        XCTAssertFalse(viewModel.notice?.message.contains("changes remain queued") == true)
    }

    func testDuplicateSendIsSuppressedWhileFirstInvocationIsInFlight() async throws {
        let controller = try authenticatedMutationController()
        let library = try await presentationLibrary(status: .active)
        let feature = FakeMetadataMutationFeature()
        let gate = QueueGate()
        feature.sendGate = gate
        let viewModel = MutationActivityViewModel(
            library: library, feature: feature, sessionController: controller)
        await viewModel.load()
        let first = Task { await viewModel.sendPendingChanges() }
        await gate.wait()
        await viewModel.sendPendingChanges()
        XCTAssertEqual(feature.sendCount, 1)
        await gate.release()
        await first.value
        XCTAssertEqual(feature.sendCount, 1)
    }

    func testUnknownRetryAtLimitDoesNotInvokeReconciliation() async throws {
        let controller = try authenticatedMutationController()
        let library = try await presentationLibrary(status: .active)
        let feature = FakeMetadataMutationFeature()
        feature.activityResult = .loaded([
            MetadataMutationActivityItem(
                id: queueUUID(555), kind: .renameNode, targetLabel: queueUUID(20),
                state: .outcomeUnknown, enqueuedAt: Date(), lastAttemptAt: Date(),
                conflictReason: nil, recoveryAttemptCount: 8, mayCheckRestore: false)
        ])
        let viewModel = MutationActivityViewModel(
            library: library, feature: feature, sessionController: controller)
        await viewModel.load()
        await viewModel.reconcileUnknown(mutationId: queueUUID(555))
        XCTAssertEqual(feature.reconcileCount, 0)
        XCTAssertEqual(viewModel.notice?.title, "Retry unavailable")
    }

    func testActivityReadAndSetupDoNotImplicitlyInitializeCheckpoint() async throws {
        let controller = try authenticatedMutationController()
        let library = try await presentationLibrary(status: .active)
        let feature = FakeMetadataMutationFeature()
        feature.availabilityResult = .setupRequired
        let viewModel = MutationActivityViewModel(
            library: library, feature: feature, sessionController: controller)
        await viewModel.load()
        XCTAssertEqual(feature.setupCount, 0)
        XCTAssertEqual(feature.sendCount, 0)
        await viewModel.enableChanges()
        XCTAssertEqual(feature.setupCount, 1)
        XCTAssertEqual(feature.sendCount, 0)
    }

    func testSessionChangeInvalidatesActivityAndPreventsLateUserAction() async throws {
        let controller = try authenticatedMutationController()
        let library = try await presentationLibrary(status: .active)
        let feature = FakeMetadataMutationFeature()
        feature.activityResult = .loaded([
            MetadataMutationActivityItem(
                id: queueUUID(556), kind: .renameNode, targetLabel: "private-name",
                state: .pending, enqueuedAt: Date(), lastAttemptAt: nil,
                conflictReason: nil, recoveryAttemptCount: nil, mayCheckRestore: false)
        ])
        let viewModel = MutationActivityViewModel(
            library: library, feature: feature, sessionController: controller)
        await viewModel.load()
        XCTAssertEqual(viewModel.items.count, 1)

        controller.requireRecovery(.authentication)
        viewModel.sessionDidChange()
        await viewModel.sendPendingChanges()

        XCTAssertEqual(viewModel.state, .invalidated)
        XCTAssertTrue(viewModel.items.isEmpty)
        XCTAssertEqual(feature.sendCount, 0)
    }

    func testNativeMutationViewsCanBeConstructedWithInjectedFacade() async throws {
        let controller = try authenticatedMutationController()
        let library = try await presentationLibrary(status: .active)
        let route = NodeBrowserRoute(
            library: NodeBrowserLibraryContext(
                id: library.id, name: library.name, rootNodeId: library.rootNodeId,
                status: library.status),
            parentScope: .libraryRoot(rootNodeId: library.rootNodeId),
            directoryTitle: library.name, ancestry: [library.rootNodeId])
        let feature = FakeMetadataMutationFeature()
        let browser = NodeBrowserView(
            repository: nil, sessionController: controller, route: route,
            metadataMutationFeature: feature)
        let activity = MutationActivityView(
            library: library, feature: feature, sessionController: controller)
        let editor = MetadataNameEditorView(
            title: "Rename", initialName: "old name", fieldLabel: "New name",
            helpText: "Exact spelling is preserved."
        ) { _ in true }

        XCTAssertNotNil(browser.body)
        XCTAssertNotNil(activity.body)
        XCTAssertNotNil(editor.body)
    }

    private func makeFeature(
        _ fixture: QueueFixture,
        repository: MutationNodeRepository,
        generator: CountingMutationIDGenerator,
        queue: DurableMutationQueue? = nil
    ) -> MetadataMutationService {
        let serviceQueue = queue ?? fixture.queue
        let coordinator = MutationDrainCoordinator(
            queue: serviceQueue, provider: fixture.provider, bridge: QueueValidator())
        return MetadataMutationService(
            provider: fixture.provider, queue: serviceQueue,
            checkpointService: fixture.service, coordinator: coordinator,
            nodeRepository: repository, bridge: QueueValidator(), identityGenerator: generator)
    }

    private func mutationLibrary(
        _ fixture: QueueFixture, root: NodeId, status: LibraryStatus = .active
    ) -> MetadataMutationLibraryContext {
        MetadataMutationLibraryContext(
            id: fixture.scope.libraryId, name: "Queue test", rootNodeId: root, status: status)
    }

    private func mutationNode(
        _ number: Int,
        scope: ClientMutationScope,
        libraryId: LibraryId? = nil,
        parent: NodeId?,
        revision: String = "3",
        kind: NodeKind = .file,
        state: NodeState = .active
    ) async throws -> Node {
        Node(
            id: try await NodeId.validated(queueUUID(number), using: QueueValidator()),
            libraryId: libraryId ?? scope.libraryId,
            parentId: parent, currentVersionId: nil,
            revision: try NodeRevision(validating: revision), name: "Node \(number)",
            kind: kind, state: state, createdAt: Date(timeIntervalSince1970: 1_760_000_000),
            updatedAt: Date(timeIntervalSince1970: 1_760_000_100), trashedAt: nil,
            restoreDeadline: nil, purgeEligible: false)
    }

    private func appliedTrashResponse(
        _ mutation: PreparedClientMutation, source: Node
    ) throws -> HTTPTransportResponse {
        let object: [String: Any] = [
            "data": [
                "outcome": "APPLIED", "mutation_id": mutation.id.rawValue,
                "kind": "TRASH_NODE", "replayed": false,
                "journal_event_id": queueUUID(590), "journal_sequence": "8",
                "node": [
                    "id": source.id.rawValue, "library_id": source.libraryId.rawValue,
                    "parent_node_id": source.parentId!.rawValue,
                    "kind": source.kind.rawValue, "state": "TRASHED", "name": source.name,
                    "revision": "6", "trashed_at": "2026-10-09T12:00:00Z",
                    "created_at": "2026-10-09T11:00:00Z", "updated_at": "2026-10-09T12:00:00Z",
                ],
            ],
            "meta": ["request_id": "mutation-request-01"],
        ]
        return try queueHTTP(object)
    }

    private func presentationLibrary(status: LibraryStatus) async throws
        -> MetadataMutationLibraryContext
    {
        let libraryId = try await LibraryId.validated(queueUUID(80), using: QueueValidator())
        let root = try await NodeId.validated(queueUUID(81), using: QueueValidator())
        return MetadataMutationLibraryContext(
            id: libraryId, name: "Mutation tests", rootNodeId: root, status: status)
    }

    private func authenticatedMutationController() throws -> SessionController {
        let endpoint = try ServerEndpoint(validating: "https://mutation-tests.example")
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: endpoint))
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(endpoint)
        controller.requireEnrollment()
        let record = try DeviceCredentialRecord(
            ownerUserId: queueUUID(90), deviceId: queueUUID(91), credentialId: queueUUID(92),
            credential: DeviceCredential(validatedRawValue: queueBearer),
            createdAt: "2026-10-09T12:00:00Z")
        controller.markAuthenticated(
            after: SecureCredentialPersistenceReceipt(
                session: DeviceCredentialSession(serverEndpoint: endpoint, record: record)))
        return controller
    }
}

private actor CountingMutationIDGenerator: ClientMutationIdentityGeneratorProtocol {
    private var next: Int
    private var count = 0
    init(first: Int) { next = first }
    func generateClientMutationID() async throws -> String {
        count += 1
        defer { next += 1 }
        return queueUUID(next)
    }
    func generatedCount() -> Int { count }
}

@MainActor
private final class MutationNodeRepository: NodeRepositoryProtocol {
    private var nodes: [Node]
    private let metadataGate: QueueGate?
    private(set) var metadataRequestCount = 0

    init(nodes: [Node], metadataGate: QueueGate? = nil) {
        self.nodes = nodes
        self.metadataGate = metadataGate
    }

    func replace(_ node: Node) {
        nodes.removeAll { $0.id == node.id && $0.libraryId == node.libraryId }
        nodes.append(node)
    }

    func listChildren(libraryId: LibraryId, parent: NodeParentScope) async -> NodeRepositoryResult {
        .loaded(
            nodes.filter {
                $0.libraryId == libraryId && $0.parentId == parent.expectedParentId
                    && $0.state == .active
            })
    }

    func getNode(libraryId: LibraryId, nodeId: NodeId, expectedParent: NodeParentScope) async
        -> NodeDetailsRepositoryResult
    {
        guard
            let node = nodes.first(where: {
                $0.id == nodeId && $0.libraryId == libraryId
                    && $0.parentId == expectedParent.expectedParentId
            })
        else { return .unavailable }
        return .loaded(node)
    }

    func getNodeMetadata(libraryId: LibraryId, nodeId: NodeId) async
        -> NodeMetadataRepositoryResult
    {
        metadataRequestCount += 1
        if let metadataGate { await metadataGate.arrive() }
        guard let node = nodes.first(where: { $0.id == nodeId && $0.libraryId == libraryId })
        else { return .unavailable }
        return .loaded(node)
    }
}

@MainActor
private final class FakeMetadataMutationFeature: MetadataMutationFeatureProtocol {
    var availabilityResult: MetadataMutationAvailability = .ready
    var folderParentAvailabilityResult: MetadataMutationAvailability = .ready
    var activityResult: MetadataMutationActivityResult = .loaded([])
    var enqueueResult: MetadataMutationEnqueueResult = .failed(.invalidMetadata)
    var drainResult = MetadataMutationDrainPresentation(
        attempted: 1, applied: 1, conflicts: 0, unknown: 0, permanentRejections: 0,
        rebaselineBlocked: 0, localFailures: 0, stop: nil)
    var sendGate: QueueGate?
    private(set) var sendCount = 0
    private(set) var setupCount = 0
    private(set) var reconcileCount = 0

    func availability(in library: MetadataMutationLibraryContext) async
        -> MetadataMutationAvailability
    { availabilityResult }
    func folderParentAvailability(
        in library: MetadataMutationLibraryContext,
        parentNodeId: NodeId,
        parentSnapshot: Node?,
        forceRefresh: Bool
    ) async -> MetadataMutationAvailability { folderParentAvailabilityResult }
    func enableChanges(in library: MetadataMutationLibraryContext) async
        -> MetadataMutationSetupResult
    {
        setupCount += 1
        availabilityResult = .ready
        return .prepared
    }
    func enqueue(_ command: MetadataMutationCommand, in library: MetadataMutationLibraryContext)
        async
        -> MetadataMutationEnqueueResult
    { enqueueResult }
    func activity(in library: MetadataMutationLibraryContext, limit: Int) async
        -> MetadataMutationActivityResult
    { activityResult }
    func sendPendingChanges(in library: MetadataMutationLibraryContext) async
        -> MetadataMutationDrainPresentation
    {
        sendCount += 1
        if let sendGate { await sendGate.arrive() }
        return drainResult
    }
    func checkRestore(trashedOperationId: String, in library: MetadataMutationLibraryContext) async
        -> MetadataMutationFailure?
    { nil }
    func enqueueRestore(trashedOperationId: String, in library: MetadataMutationLibraryContext)
        async
        -> MetadataMutationEnqueueResult
    { enqueueResult }
    func reconcileUnknown(mutationId: String, in library: MetadataMutationLibraryContext) async
        -> MetadataMutationDrainPresentation
    {
        reconcileCount += 1
        return drainResult
    }
}
