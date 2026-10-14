import Foundation

enum NotificationDestination {
    case file(NodeFileDetailsRoute)
    case browser(NodeBrowserRoute)
    case synchronization(Library)
    case unavailable
}

@MainActor
protocol NotificationAuthorityProtocol {
    func currentScope() async throws -> String
    func verify(_ candidate: NotificationCandidate) async throws -> Bool
    func destination(_ candidate: NotificationCandidate) async throws -> NotificationDestination
}

/// Read-only adapter to the existing authorities. No transfer, mutation, ACK or sync method is
/// available through NotificationAuthorityProtocol. Scope acquisition always reloads Keychain.
@MainActor
final class NotificationAuthority: NotificationAuthorityProtocol {
    private let provider: any LocalLibrarySessionProviderProtocol
    private let database: MutationQueueSQLiteStore
    private let bridge: any RustBridgeProtocol
    private let downloads: FileDownloadCoordinator?
    private let catalog: any LibraryCatalogRepositoryProtocol
    private let nodes: any NodeRepositoryProtocol
    init(
        provider: any LocalLibrarySessionProviderProtocol, database: MutationQueueSQLiteStore,
        bridge: any RustBridgeProtocol, downloads: FileDownloadCoordinator?,
        catalog: any LibraryCatalogRepositoryProtocol, nodes: any NodeRepositoryProtocol
    ) {
        self.provider = provider
        self.database = database
        self.bridge = bridge
        self.downloads = downloads
        self.catalog = catalog
        self.nodes = nodes
    }
    func currentScope() async throws -> String {
        let session = try await provider.begin()
        try await provider.validate(session)
        return NotificationFingerprint.scope(session)
    }
    private func capture(_ candidate: NotificationCandidate) async throws
        -> (LibraryRequestScope, ClientMutationScope)
    {
        guard candidate.isValid else { throw NotificationFailure.payloadRejected }
        let session = try await provider.begin()
        guard NotificationFingerprint.scope(session) == candidate.scopeFingerprint else {
            throw NotificationFailure.accountMismatch
        }
        let library = try await LibraryId.validated(candidate.library, using: bridge)
        let scope = try await session.mutationScope(libraryId: library, bridge: bridge)
        try await provider.validate(session)
        return (session, scope)
    }
    func verify(_ candidate: NotificationCandidate) async throws -> Bool {
        let (session, scope) = try await capture(candidate)
        let observed: NotificationCandidate?
        switch candidate.evidence {
        case .verifiedDownload:
            guard
                let record = try await database.download(
                    scope: scope, credentialId: session.credentialIdentifier,
                    id: DownloadOperationId(validating: candidate.operation), bridge: bridge),
                let downloads
            else { return false }
            observed = NotificationEventClassifier.download(
                record, scope: candidate.scopeFingerprint)
            guard observed == candidate else { return false }
            try await downloads.validateNotificationContent(record, session: session)
        case .committedUpload, .uncertainUpload:
            guard
                let record = try await database.upload(
                    scope: scope, credentialId: session.credentialIdentifier,
                    id: UploadOperationId(validating: candidate.operation), bridge: bridge)
            else { return false }
            observed = NotificationEventClassifier.upload(record, scope: candidate.scopeFingerprint)
        case .confirmedInbound, .unknownAck:
            let record: InboundSyncPageRecord?
            if candidate.evidence == .confirmedInbound {
                record = try await database.latestConfirmedInbound(
                    scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
            } else {
                record = try await database.oldestUnresolvedInbound(
                    scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
            }
            observed = record.flatMap {
                NotificationEventClassifier.inbound(
                    $0, scope: candidate.scopeFingerprint,
                    recovery: candidate.evidence == .unknownAck)
            }
        }
        try await provider.validate(session)
        return observed == candidate
    }
    func destination(_ candidate: NotificationCandidate) async throws -> NotificationDestination {
        let (session, scope) = try await capture(candidate)
        guard try await verify(candidate),
            case .loaded(let libraries) = await catalog.listLibraries(),
            let library = libraries.first(where: { $0.id == scope.libraryId }),
            library.status != .quarantined
        else { return .unavailable }
        try await provider.validate(session)
        if candidate.evidence == .unknownAck || candidate.evidence == .confirmedInbound {
            return .synchronization(library)
        }
        if candidate.evidence == .uncertainUpload { return .browser(.root(for: library)) }
        let nodeId: NodeId
        if candidate.evidence == .verifiedDownload {
            guard
                let record = try await database.download(
                    scope: scope, credentialId: session.credentialIdentifier,
                    id: DownloadOperationId(validating: candidate.operation), bridge: bridge)
            else { return .unavailable }
            nodeId = record.identity.nodeId
        } else {
            guard
                let record = try await database.upload(
                    scope: scope, credentialId: session.credentialIdentifier,
                    id: UploadOperationId(validating: candidate.operation), bridge: bridge),
                let completion = record.completion
            else { return .unavailable }
            nodeId = completion.nodeId
        }
        guard
            case .loaded(let node) = await nodes.getNodeMetadata(
                libraryId: library.id, nodeId: nodeId), node.id == nodeId,
            node.libraryId == library.id, node.kind == .file, node.state == .active,
            node.trashedAt == nil, node.restoreDeadline == nil, !node.purgeEligible,
            let parent = node.parentId
        else { return .unavailable }
        try await provider.validate(session)
        let parentScope: NodeParentScope =
            parent == library.rootNodeId
            ? .libraryRoot(rootNodeId: parent) : .directory(parent)
        return .file(
            NodeFileDetailsRoute(
                node: node, library: NodeBrowserLibraryContext(library), parentScope: parentScope,
                ancestry: [parent], parentDirectoryTitle: "File"))
    }

    func observeDownload(_ record: DownloadRecord) async throws -> NotificationCandidate? {
        let session = try await provider.begin()
        guard session.matches(record.identity.scope),
            session.credentialIdentifier == record.identity.credentialId
        else { throw NotificationFailure.accountMismatch }
        try await provider.validate(session)
        return NotificationEventClassifier.download(
            record, scope: NotificationFingerprint.scope(session))
    }
    func observeUpload(_ record: UploadRecord) async throws -> NotificationCandidate? {
        let session = try await provider.begin()
        guard session.matches(record.scope), session.credentialIdentifier == record.credentialId
        else {
            throw NotificationFailure.accountMismatch
        }
        try await provider.validate(session)
        return NotificationEventClassifier.upload(
            record, scope: NotificationFingerprint.scope(session))
    }
    func observeInbound(library: LibraryId, recovery: Bool) async throws -> NotificationCandidate? {
        let session = try await provider.begin()
        let scope = try await session.mutationScope(libraryId: library, bridge: bridge)
        let record: InboundSyncPageRecord?
        if recovery {
            record = try await database.oldestUnresolvedInbound(
                scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
        } else {
            record = try await database.latestConfirmedInbound(
                scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
        }
        try await provider.validate(session)
        return record.flatMap {
            NotificationEventClassifier.inbound(
                $0, scope: NotificationFingerprint.scope(session), recovery: recovery)
        }
    }
}
