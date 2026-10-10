import Foundation

/// Non-secret Library identity and status carried from the validated catalog into the feature.
struct MetadataMutationLibraryContext: Equatable, Sendable {
    let id: LibraryId
    let name: String
    let rootNodeId: NodeId
    let status: LibraryStatus
}

enum MetadataMutationUnavailableReason: Equatable, Sendable {
    case libraryReadOnly
    case libraryQuarantined
    case sessionUnavailable
    case queueUnavailable
    case recoveryRequired
    case invalidMetadata
}

enum MetadataMutationAvailability: Equatable, Sendable {
    case ready
    case setupRequired
    case unavailable(MetadataMutationUnavailableReason)
}

enum MetadataMutationFailure: Equatable, Sendable {
    case readOnlyLibrary
    case quarantinedLibrary
    case sessionUnavailable
    case queueUnavailable
    case syncBaseRequired
    case recoveryRequired
    case invalidMetadata
    case invalidName
    case confirmationRequired
    case duplicateSubmission
    case operationUnavailable
    case retryLimitReached
    case commitAcknowledgementUncertain
    case resultPersistenceUncertain
    case conflictNeedsReview
    case unknownNeedsReconciliation
    case permanentRejection
    case offline
    case authorizationRequired
    case serverUnavailable
    case transportUnavailable
}

struct MetadataMutationReceipt: Equatable, Sendable {
    let mutationId: String
    let kind: ClientMutationKind
    let state: MutationQueueState
}

enum MetadataMutationEnqueueResult: Equatable, Sendable {
    case persisted(MetadataMutationReceipt)
    case failed(MetadataMutationFailure)
}

enum MetadataMutationSetupResult: Equatable, Sendable {
    case prepared
    case alreadyPrepared
    case failed(MetadataMutationFailure)
}

struct MetadataMutationActivityItem: Equatable, Sendable, Identifiable {
    let id: String
    let kind: ClientMutationKind
    let targetLabel: String
    let state: MutationQueueState
    let enqueuedAt: Date
    let lastAttemptAt: Date?
    let conflictReason: ClientMutationConflictReason?
    let recoveryAttemptCount: Int?
    let mayCheckRestore: Bool
}

enum MetadataMutationActivityResult: Equatable, Sendable {
    case loaded([MetadataMutationActivityItem])
    case failed(MetadataMutationFailure)
}

struct MetadataMutationDrainPresentation: Equatable, Sendable {
    let attempted: Int
    let applied: Int
    let conflicts: Int
    let unknown: Int
    let permanentRejections: Int
    let rebaselineBlocked: Int
    let localFailures: Int
    let stop: MetadataMutationFailure?
}

enum MetadataMutationCommand: Equatable, Sendable {
    case createFolder(
        parentNodeId: NodeId, parentAncestry: [NodeId], parentSnapshot: Node?, name: String)
    case rename(node: Node, newName: String)
    case move(node: Node, destination: Node, destinationAncestry: [NodeId])
    case trash(node: Node, confirmed: Bool)
}

@MainActor
protocol MetadataMutationFeatureProtocol {
    func availability(in library: MetadataMutationLibraryContext) async
        -> MetadataMutationAvailability
    func folderParentAvailability(
        in library: MetadataMutationLibraryContext,
        parentNodeId: NodeId,
        parentSnapshot: Node?,
        forceRefresh: Bool
    ) async -> MetadataMutationAvailability
    func enableChanges(in library: MetadataMutationLibraryContext) async
        -> MetadataMutationSetupResult
    func enqueue(
        _ command: MetadataMutationCommand,
        in library: MetadataMutationLibraryContext
    ) async -> MetadataMutationEnqueueResult
    func activity(in library: MetadataMutationLibraryContext, limit: Int) async
        -> MetadataMutationActivityResult
    func sendPendingChanges(in library: MetadataMutationLibraryContext) async
        -> MetadataMutationDrainPresentation
    func checkRestore(
        trashedOperationId: String,
        in library: MetadataMutationLibraryContext
    ) async -> MetadataMutationFailure?
    func enqueueRestore(
        trashedOperationId: String,
        in library: MetadataMutationLibraryContext
    ) async -> MetadataMutationEnqueueResult
    func reconcileUnknown(
        mutationId: String,
        in library: MetadataMutationLibraryContext
    ) async -> MetadataMutationDrainPresentation
}

/// Narrow SwiftUI-facing facade. It owns validation and preparation and never exposes the queue,
/// submission repository, leases, credentials, or arbitrary HTTP composition.
@MainActor
final class MetadataMutationService: MetadataMutationFeatureProtocol {
    private let provider: any AuthenticatedSyncCheckpointRequestProviderProtocol
    private let queue: DurableMutationQueue
    private let checkpointService: SyncCheckpointService
    private let coordinator: MutationDrainCoordinator
    private let nodeRepository: any NodeRepositoryProtocol
    private let bridge: any RustBridgeProtocol
    private let identityGenerator: any ClientMutationIdentityGeneratorProtocol
    private var activeCommands: Set<String> = []
    private var unacknowledgedEnqueues: [String: PreparedClientMutation] = [:]
    private var verifiedParentSnapshots: [(ClientMutationScope, LibraryRequestScope, Node)] = []
    private var requiresFreshParentReadScopes: [ClientMutationScope] = []

    init(
        provider: any AuthenticatedSyncCheckpointRequestProviderProtocol,
        queue: DurableMutationQueue,
        checkpointService: SyncCheckpointService,
        coordinator: MutationDrainCoordinator,
        nodeRepository: any NodeRepositoryProtocol,
        bridge: any RustBridgeProtocol,
        identityGenerator: any ClientMutationIdentityGeneratorProtocol
    ) {
        self.provider = provider
        self.queue = queue
        self.checkpointService = checkpointService
        self.coordinator = coordinator
        self.nodeRepository = nodeRepository
        self.bridge = bridge
        self.identityGenerator = identityGenerator
    }

    func availability(in library: MetadataMutationLibraryContext) async
        -> MetadataMutationAvailability
    {
        guard library.status == .active else {
            return .unavailable(
                library.status == .readOnly ? .libraryReadOnly : .libraryQuarantined)
        }
        do {
            let scope = try await makeScope(in: library)
            _ = try await queue.persistedBase(scope: scope)
            return .ready
        } catch {
            switch DurableMutationQueue.classify(error) {
            case .syncBaseUnavailable:
                return .setupRequired
            case .reconciliationRequired, .invalidCheckpoint:
                return .unavailable(.recoveryRequired)
            case .unauthenticated, .staleSession, .committedButSessionChanged:
                return .unavailable(.sessionUnavailable)
            default:
                return .unavailable(.queueUnavailable)
            }
        }
    }

    /// Remove transient metadata and save identities when the authenticated session ends.
    func invalidateSession() {
        verifiedParentSnapshots.removeAll()
        requiresFreshParentReadScopes.removeAll()
        unacknowledgedEnqueues.removeAll()
        activeCommands.removeAll()
    }

    func folderParentAvailability(
        in library: MetadataMutationLibraryContext,
        parentNodeId: NodeId,
        parentSnapshot: Node?,
        forceRefresh: Bool = false
    ) async -> MetadataMutationAvailability {
        guard library.status == .active else {
            return .unavailable(
                library.status == .readOnly ? .libraryReadOnly : .libraryQuarantined)
        }
        do {
            let (scope, session) = try await makeSessionScope(in: library)
            _ = try await queue.persistedBase(scope: scope)
            if forceRefresh {
                verifiedParentSnapshots.removeAll {
                    $0.0 == scope && $0.2.id == parentNodeId
                }
            }
            let needsFreshRead = requiresFreshParentReadScopes.contains(scope)
            let cached = verifiedParentSnapshots.first {
                $0.0 == scope && $0.1.sameSession(as: session) && $0.2.id == parentNodeId
            }?.2
            if !forceRefresh, let cached,
                isUsableParent(cached, id: parentNodeId, libraryId: library.id)
            {
                return .ready
            }
            if !forceRefresh, !needsFreshRead, let parentSnapshot,
                isUsableParent(parentSnapshot, id: parentNodeId, libraryId: library.id)
            {
                cacheParentSnapshot(parentSnapshot, scope: scope, session: session)
                try await provider.validate(session)
                return .ready
            }
            let result = await nodeRepository.getNodeMetadata(
                libraryId: library.id, nodeId: parentNodeId)
            try await provider.validate(session)
            guard case .loaded(let parent) = result,
                isUsableParent(parent, id: parentNodeId, libraryId: library.id)
            else { return .unavailable(.invalidMetadata) }
            cacheParentSnapshot(parent, scope: scope, session: session)
            return .ready
        } catch {
            switch DurableMutationQueue.classify(error) {
            case .syncBaseUnavailable: return .setupRequired
            case .reconciliationRequired, .invalidCheckpoint:
                return .unavailable(.recoveryRequired)
            case .unauthenticated, .staleSession, .committedButSessionChanged:
                return .unavailable(.sessionUnavailable)
            default: return .unavailable(.queueUnavailable)
            }
        }
    }

    func enableChanges(in library: MetadataMutationLibraryContext) async
        -> MetadataMutationSetupResult
    {
        guard library.status == .active else {
            return .failed(library.status == .readOnly ? .readOnlyLibrary : .quarantinedLibrary)
        }
        do {
            let scope = try await makeScope(in: library)
            do {
                _ = try await queue.persistedBase(scope: scope)
                return .alreadyPrepared
            } catch let failure as MutationQueueFailure where failure == .syncBaseUnavailable {
                // The one allowed setup case falls through to the explicit checkpoint GET.
            } catch {
                return .failed(map(DurableMutationQueue.classify(error)))
            }
            switch await checkpointService.prepare(scope: scope) {
            case .prepared: return .prepared
            case .failed(let failure): return .failed(map(failure))
            }
        } catch { return .failed(map(DurableMutationQueue.classify(error))) }
    }

    func enqueue(
        _ command: MetadataMutationCommand,
        in library: MetadataMutationLibraryContext
    ) async -> MetadataMutationEnqueueResult {
        guard library.status == .active else {
            return .failed(library.status == .readOnly ? .readOnlyLibrary : .quarantinedLibrary)
        }
        let key = commandKey(command, library: library)
        guard activeCommands.insert(key).inserted else { return .failed(.duplicateSubmission) }
        defer { activeCommands.remove(key) }
        do {
            let (scope, session) = try await makeSessionScope(in: library)
            if let previous = unacknowledgedEnqueues[key] {
                guard previous.base.scope == scope else {
                    return .failed(.sessionUnavailable)
                }
                return await enqueuePrepared(previous, key: key)
            }
            let base = try await queue.persistedBase(scope: scope)
            let intent = try await prepareIntent(
                command, library: library, scope: scope, session: session)
            let id = try await ClientMutationId.generate(
                using: identityGenerator, validator: bridge)
            let mutation = try await PreparedClientMutation(
                id: id, base: base, intent: intent, bridge: bridge)
            return await enqueuePrepared(mutation, key: key)
        } catch let failure as ClientMutationFailure {
            return .failed(failure == .invalidPreparation ? .invalidName : map(failure))
        } catch let failure as MetadataMutationFailure {
            return .failed(failure)
        } catch { return .failed(map(DurableMutationQueue.classify(error))) }
    }

    func activity(in library: MetadataMutationLibraryContext, limit: Int) async
        -> MetadataMutationActivityResult
    {
        guard
            library.status == .active || library.status == .readOnly
                || library.status == .quarantined
        else { return .failed(.invalidMetadata) }
        guard (1...MutationQueuePolicy.maximumReadBatch).contains(limit) else {
            return .failed(.queueUnavailable)
        }
        do {
            let scope = try await makeScope(in: library)
            let records: [MutationQueueRecord]
            switch await queue.activity(scope: scope, limit: limit) {
            case .failed(let failure): return .failed(map(failure))
            case .records(let values): records = values
            }
            var items: [MetadataMutationActivityItem] = []
            for record in records {
                var conflictReason: ClientMutationConflictReason?
                var mayCheckRestore = false
                if let evidence = record.evidence, let body = evidence.responseBody,
                    let status = evidence.responseStatus
                {
                    let result = try? await ClientMutationResponseDecoder(bridge: bridge).decode(
                        HTTPTransportResponse(
                            statusCode: status,
                            headers: ["Content-Type": "application/json"], body: body),
                        for: record.mutation)
                    if case .some(.conflict(let conflict?, _)) = result {
                        conflictReason = conflict.reason
                    }
                    if case .some(.applied(let applied)) = result {
                        mayCheckRestore =
                            record.state == .applied
                            && record.mutation.kind == .trashNode
                            && applied.node.state == .trashed
                            && applied.node.parentNodeId != nil
                    }
                }
                var recoveryCount: Int?
                if record.state == .outcomeUnknown {
                    switch await queue.recoveryAttemptCount(
                        scope: scope, mutationId: record.mutation.id)
                    {
                    case .count(let value): recoveryCount = value
                    case .missing, .failed: recoveryCount = nil
                    }
                }
                items.append(
                    MetadataMutationActivityItem(
                        id: record.mutation.id.rawValue, kind: record.mutation.kind,
                        targetLabel: targetLabel(for: record.mutation), state: record.state,
                        enqueuedAt: record.createdAt, lastAttemptAt: record.attempt?.startedAt,
                        conflictReason: conflictReason, recoveryAttemptCount: recoveryCount,
                        mayCheckRestore: mayCheckRestore))
            }
            return .loaded(items)
        } catch { return .failed(map(DurableMutationQueue.classify(error))) }
    }

    func sendPendingChanges(in library: MetadataMutationLibraryContext) async
        -> MetadataMutationDrainPresentation
    {
        guard library.status == .active else {
            return emptyDrain(
                map(library.status == .readOnly ? .readOnlyLibrary : .quarantinedLibrary))
        }
        do {
            let scope = try await makeScope(in: library)
            _ = try await queue.persistedBase(scope: scope)
            let presentation = present(
                await coordinator.drain(scope: scope, maximumOperations: 10))
            if presentation.applied > 0 { invalidateParentSnapshotsAfterApplied(in: scope) }
            return presentation
        } catch { return emptyDrain(map(DurableMutationQueue.classify(error))) }
    }

    func checkRestore(
        trashedOperationId: String,
        in library: MetadataMutationLibraryContext
    ) async -> MetadataMutationFailure? {
        do {
            _ = try await restoreIntent(
                operationId: trashedOperationId, in: library, requireBase: true)
            return nil
        } catch let failure as MetadataMutationFailure { return failure } catch {
            return map(DurableMutationQueue.classify(error))
        }
    }

    func enqueueRestore(
        trashedOperationId: String,
        in library: MetadataMutationLibraryContext
    ) async -> MetadataMutationEnqueueResult {
        guard library.status == .active else {
            return .failed(library.status == .readOnly ? .readOnlyLibrary : .quarantinedLibrary)
        }
        let key = "RESTORE|\(library.id.rawValue)|\(trashedOperationId)"
        guard activeCommands.insert(key).inserted else { return .failed(.duplicateSubmission) }
        defer { activeCommands.remove(key) }
        do {
            let scope = try await makeScope(in: library)
            if let previous = unacknowledgedEnqueues[key] {
                guard previous.base.scope == scope else {
                    return .failed(.sessionUnavailable)
                }
                return await enqueuePrepared(previous, key: key)
            }
            let (base, intent) = try await restoreIntent(
                operationId: trashedOperationId, in: library, requireBase: true)
            let id = try await ClientMutationId.generate(
                using: identityGenerator, validator: bridge)
            let mutation = try await PreparedClientMutation(
                id: id, base: base, intent: intent, bridge: bridge)
            return await enqueuePrepared(mutation, key: key)
        } catch let failure as MetadataMutationFailure { return .failed(failure) } catch {
            return .failed(map(DurableMutationQueue.classify(error)))
        }
    }

    func reconcileUnknown(
        mutationId: String,
        in library: MetadataMutationLibraryContext
    ) async -> MetadataMutationDrainPresentation {
        guard library.status == .active else {
            return emptyDrain(
                map(library.status == .readOnly ? .readOnlyLibrary : .quarantinedLibrary))
        }
        do {
            let scope = try await makeScope(in: library)
            let id = try await ClientMutationId.validated(mutationId, using: bridge)
            switch await queue.recoveryAttemptCount(scope: scope, mutationId: id) {
            case .count(let count) where count < MutationQueuePolicy.maximumRecoveryAttempts:
                break
            case .count: return emptyDrain(.retryLimitReached)
            case .missing: return emptyDrain(.operationUnavailable)
            case .failed(let failure): return emptyDrain(map(failure))
            }
            let presentation = present(
                await coordinator.reconcileUnknown(scope: scope, mutationId: id))
            if presentation.applied > 0 { invalidateParentSnapshotsAfterApplied(in: scope) }
            return presentation
        } catch { return emptyDrain(map(DurableMutationQueue.classify(error))) }
    }

    private func makeScope(in library: MetadataMutationLibraryContext) async throws
        -> ClientMutationScope
    {
        let (scope, _) = try await makeSessionScope(in: library)
        return scope
    }

    private func makeSessionScope(in library: MetadataMutationLibraryContext) async throws
        -> (ClientMutationScope, LibraryRequestScope)
    {
        guard
            library.status == .active || library.status == .readOnly
                || library.status == .quarantined
        else { throw MetadataMutationFailure.invalidMetadata }
        let session = try await provider.begin()
        let scope = try await session.mutationScope(libraryId: library.id, bridge: bridge)
        try await provider.validate(session)
        verifiedParentSnapshots.removeAll { !$0.1.sameSession(as: session) }
        return (scope, session)
    }

    private func prepareIntent(
        _ command: MetadataMutationCommand,
        library: MetadataMutationLibraryContext,
        scope: ClientMutationScope,
        session: LibraryRequestScope
    ) async throws -> ClientMutationIntent {
        switch command {
        case .createFolder(let parentId, let parentAncestry, let parentSnapshot, let name):
            guard parentId == library.rootNodeId || parentAncestry.contains(parentId) else {
                throw MetadataMutationFailure.invalidMetadata
            }
            guard try await bridge.validateLogicalName(name) else {
                throw MetadataMutationFailure.invalidName
            }
            let parent: Node
            let needsFreshRead = requiresFreshParentReadScopes.contains(scope)
            if let saved = verifiedParentSnapshots.first(where: {
                $0.0 == scope && $0.1.sameSession(as: session) && $0.2.id == parentId
            }) {
                parent = saved.2
            } else if let parentSnapshot, !needsFreshRead,
                isUsableParent(parentSnapshot, id: parentId, libraryId: library.id)
            {
                parent = parentSnapshot
                cacheParentSnapshot(parent, scope: scope, session: session)
            } else {
                let metadata = await nodeRepository.getNodeMetadata(
                    libraryId: library.id, nodeId: parentId)
                try await provider.validate(session)
                guard case .loaded(let verified) = metadata else {
                    throw MetadataMutationFailure.invalidMetadata
                }
                parent = verified
                cacheParentSnapshot(parent, scope: scope, session: session)
            }
            guard parent.id == parentId, parent.libraryId == library.id,
                parent.kind == .directory, parent.state == .active,
                parent.trashedAt == nil, !parent.purgeEligible
            else { throw MetadataMutationFailure.invalidMetadata }
            return .createDirectory(
                parentNodeId: parent.id, expectedParentRevision: parent.revision, name: name)

        case .rename(let node, let newName):
            guard isEditableNode(node, library: library) else {
                throw MetadataMutationFailure.invalidMetadata
            }
            guard try await bridge.validateLogicalName(newName) else {
                throw MetadataMutationFailure.invalidName
            }
            return .renameNode(nodeId: node.id, expectedRevision: node.revision, newName: newName)

        case .move(let node, let destination, let destinationAncestry):
            guard isEditableNode(node, library: library),
                destination.libraryId == library.id, destination.kind == .directory,
                destination.state == .active, destination.trashedAt == nil,
                destination.restoreDeadline == nil, !destination.purgeEligible,
                destination.id != node.id, node.parentId != destination.id,
                !destinationAncestry.contains(node.id)
            else { throw MetadataMutationFailure.invalidMetadata }
            return .moveNode(
                nodeId: node.id, expectedRevision: node.revision,
                newParentNodeId: destination.id,
                expectedNewParentRevision: destination.revision)

        case .trash(let node, let confirmed):
            guard confirmed else { throw MetadataMutationFailure.confirmationRequired }
            guard isEditableNode(node, library: library) else {
                throw MetadataMutationFailure.invalidMetadata
            }
            return .trashNode(nodeId: node.id, expectedRevision: node.revision)
        }
    }

    private func restoreIntent(
        operationId: String,
        in library: MetadataMutationLibraryContext,
        requireBase: Bool
    ) async throws -> (ClientMutationBase, ClientMutationIntent) {
        guard library.status == .active else { throw MetadataMutationFailure.readOnlyLibrary }
        let (scope, session) = try await makeSessionScope(in: library)
        guard let operation = try await lookup(operationId, scope: scope),
            operation.state == .applied, operation.mutation.kind == .trashNode
        else { throw MetadataMutationFailure.operationUnavailable }
        guard let evidence = operation.evidence, let body = evidence.responseBody,
            let status = evidence.responseStatus,
            case .applied(let result) = try await ClientMutationResponseDecoder(bridge: bridge)
                .decode(
                    HTTPTransportResponse(
                        statusCode: status, headers: ["Content-Type": "application/json"],
                        body: body), for: operation.mutation),
            result.node.state == .trashed, let parentId = result.node.parentNodeId,
            result.node.libraryId == library.id
        else { throw MetadataMutationFailure.operationUnavailable }
        let parentResult = await nodeRepository.getNodeMetadata(
            libraryId: library.id, nodeId: parentId)
        try await provider.validate(session)
        guard case .loaded(let parent) = parentResult,
            isUsableParent(parent, id: parentId, libraryId: library.id)
        else { throw MetadataMutationFailure.invalidMetadata }
        cacheParentSnapshot(parent, scope: scope, session: session)
        let base: ClientMutationBase
        if requireBase {
            base = try await queue.persistedBase(scope: scope)
            try await provider.validate(session)
        } else {
            base = operation.mutation.base
        }
        let intent = ClientMutationIntent.restoreNode(
            nodeId: result.node.id, expectedRevision: result.node.revision,
            expectedParentNodeId: parent.id, expectedParentRevision: parent.revision)
        try await intent.validate(using: bridge)
        return (base, intent)
    }

    private func lookup(_ operationId: String, scope: ClientMutationScope) async throws
        -> MutationQueueRecord?
    {
        let id = try await ClientMutationId.validated(operationId, using: bridge)
        switch await queue.get(scope: scope, mutationId: id) {
        case .record(let record): return record
        case .missing: return nil
        case .failed(let failure): throw failure
        }
    }

    /// Read back the original UUID before retrying a local save. A prior COMMIT with a lost
    /// acknowledgement therefore returns its existing row instead of creating a second intent.
    private func enqueuePrepared(
        _ mutation: PreparedClientMutation, key: String
    ) async -> MetadataMutationEnqueueResult {
        switch await queue.get(scope: mutation.base.scope, mutationId: mutation.id) {
        case .record(let record):
            guard record.mutation == mutation else { return .failed(.invalidMetadata) }
            unacknowledgedEnqueues.removeValue(forKey: key)
            return .persisted(
                MetadataMutationReceipt(
                    mutationId: record.mutation.id.rawValue, kind: record.mutation.kind,
                    state: record.state))
        case .failed(let failure):
            return .failed(map(failure))
        case .missing:
            switch await queue.enqueue(mutation) {
            case .enqueued(let record), .existing(let record):
                guard record.mutation == mutation else { return .failed(.invalidMetadata) }
                unacknowledgedEnqueues.removeValue(forKey: key)
                return .persisted(
                    MetadataMutationReceipt(
                        mutationId: record.mutation.id.rawValue, kind: record.mutation.kind,
                        state: record.state))
            case .failed(let failure):
                if failure == .commitAcknowledgementLost
                    || failure == .committedButSessionChanged
                {
                    unacknowledgedEnqueues[key] = mutation
                }
                return .failed(map(failure))
            }
        }
    }

    private func isEditableNode(_ node: Node, library: MetadataMutationLibraryContext) -> Bool {
        node.libraryId == library.id && node.state == .active && node.trashedAt == nil
            && node.restoreDeadline == nil && !node.purgeEligible
    }

    private func isUsableParent(
        _ node: Node, id: NodeId, libraryId: LibraryId
    ) -> Bool {
        node.id == id && node.libraryId == libraryId && node.kind == .directory
            && node.state == .active && node.trashedAt == nil && node.restoreDeadline == nil
            && !node.purgeEligible
    }

    private func cacheParentSnapshot(
        _ node: Node, scope: ClientMutationScope, session: LibraryRequestScope
    ) {
        verifiedParentSnapshots.removeAll {
            $0.0 == scope && $0.1.sameSession(as: session) && $0.2.id == node.id
        }
        verifiedParentSnapshots.append((scope, session, node))
    }

    private func invalidateParentSnapshotsAfterApplied(in scope: ClientMutationScope) {
        verifiedParentSnapshots.removeAll { $0.0 == scope }
        if !requiresFreshParentReadScopes.contains(scope) {
            requiresFreshParentReadScopes.append(scope)
        }
    }

    private func commandKey(
        _ command: MetadataMutationCommand, library: MetadataMutationLibraryContext
    ) -> String {
        switch command {
        case .createFolder(let parent, _, _, let name):
            "CREATE|\(library.id.rawValue)|\(parent.rawValue)|\(name)"
        case .rename(let node, let name):
            "RENAME|\(library.id.rawValue)|\(node.id.rawValue)|\(node.revision.rawValue)|\(name)"
        case .move(let node, let destination, _):
            "MOVE|\(library.id.rawValue)|\(node.id.rawValue)|\(node.revision.rawValue)|\(destination.id.rawValue)|\(destination.revision.rawValue)"
        case .trash(let node, let confirmed):
            "TRASH|\(library.id.rawValue)|\(node.id.rawValue)|\(node.revision.rawValue)|\(confirmed)"
        }
    }

    private func targetLabel(for mutation: PreparedClientMutation) -> String {
        switch mutation.payload.intent {
        case .createDirectory(_, _, let name): "Folder “\(name)”"
        case .renameNode(let id, _, _), .trashNode(let id, _): id.rawValue
        case .moveNode(let id, _, _, _), .restoreNode(let id, _, _, _): id.rawValue
        }
    }

    private func present(_ result: MutationDrainResult) -> MetadataMutationDrainPresentation {
        let summary: MutationDrainSummary
        let stop: MetadataMutationFailure?
        switch result {
        case .completed(let value):
            summary = value
            stop = nil
        case .stopped(let reason, let value):
            summary = value
            switch reason {
            case .unknownRequiresReconciliation: stop = .unknownNeedsReconciliation
            case .conflictRequiresReview, .mutationIdConflict: stop = .conflictNeedsReview
            case .rebaselineRequired: stop = .recoveryRequired
            case .resultPersistenceFailed: stop = .resultPersistenceUncertain
            case .queue(.recoveryLimit): stop = .retryLimitReached
            case .queue: stop = .transportUnavailable
            case .submission(.permanentRejection): stop = .permanentRejection
            case .submission: stop = .transportUnavailable
            }
        }
        return MetadataMutationDrainPresentation(
            attempted: summary.attempted, applied: summary.applied,
            conflicts: summary.conflicts + summary.identityConflicts,
            unknown: summary.unknown, permanentRejections: summary.permanentRejections,
            rebaselineBlocked: summary.blockedRebaseline,
            localFailures: summary.localPreDispatchFailures, stop: stop)
    }

    private func emptyDrain(_ failure: MetadataMutationFailure) -> MetadataMutationDrainPresentation
    {
        MetadataMutationDrainPresentation(
            attempted: 0, applied: 0, conflicts: 0, unknown: 0, permanentRejections: 0,
            rebaselineBlocked: 0, localFailures: 0, stop: failure)
    }

    private func map(_ failure: MutationQueueFailure) -> MetadataMutationFailure {
        switch failure {
        case .syncBaseUnavailable: .syncBaseRequired
        case .invalidCheckpoint, .reconciliationRequired: .recoveryRequired
        case .unauthenticated, .staleSession, .committedButSessionChanged: .sessionUnavailable
        case .commitAcknowledgementLost: .commitAcknowledgementUncertain
        case .recoveryLimit: .retryLimitReached
        case .invalidOperation, .malformedRecord, .scopeMismatch: .invalidMetadata
        case .unavailable, .databaseOpen, .busy, .diskFull, .corrupt, .io,
            .unsupportedSchema, .capacity, .dependencyConflict:
            .queueUnavailable
        case .transport(let cause):
            switch cause {
            case .offline, .dnsFailure, .timeout: .offline
            case .authenticationRejected, .deviceRevoked, .unauthenticated,
                .credentialUnavailable, .invalidCredential, .originMismatch:
                .authorizationRequired
            case .serverUnavailable: .serverUnavailable
            default: .transportUnavailable
            }
        case .cancelled: .sessionUnavailable
        case .notFound, .duplicateIdentity, .invalidTransition, .ownershipRequired:
            .operationUnavailable
        case .invalidLimit, .concurrentExecution: .duplicateSubmission
        }
    }

    private func map(_ failure: ClientMutationFailure) -> MetadataMutationFailure {
        switch failure {
        case .invalidPreparation: .invalidName
        case .scopeMismatch, .staleSession, .unauthenticated: .sessionUnavailable
        case .permanentRejection: .permanentRejection
        case .cancelled: .sessionUnavailable
        default: .transportUnavailable
        }
    }
}
