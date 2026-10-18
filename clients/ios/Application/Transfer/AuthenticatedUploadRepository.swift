import Foundation

@MainActor
final class AuthenticatedUploadRepository {
    let provider: any AuthenticatedUploadRequestProviderProtocol
    let bridge: any RustBridgeProtocol

    init(provider: any AuthenticatedUploadRequestProviderProtocol, bridge: any RustBridgeProtocol) {
        self.provider = provider
        self.bridge = bridge
    }

    func preflight(
        _ target: UploadTarget, session: LibraryRequestScope, verifyRevision: Bool = true
    ) async throws
        -> ClientMutationScope
    {
        switch await AuthenticatedLibraryCatalogRepository(
            provider: provider, bridge: bridge
        ).listLibraries()
        {
        case .loaded(let libraries):
            guard libraries.first(where: { $0.id == target.library })?.status == .active else {
                throw UploadFailure.invalidTarget
            }
        case .failed(let failure):
            throw failure
        }
        try await provider.validate(session)
        let scope = try await session.mutationScope(libraryId: target.library, bridge: bridge)
        let nodeId: NodeId
        switch target {
        case .createFile(_, let parent, _): nodeId = parent
        case .replaceContent(_, let node, _): nodeId = node
        }
        _ = try await NodeId.validated(nodeId.rawValue, using: bridge)
        let response = try await provider.requestNode(nodeId: nodeId, scope: session)
        try AuthenticatedLibraryCatalogRepository.validateHTTP(response)
        let node = try await NodeResponseDecoder(bridge: bridge).decodeSingle(response.body)
        try await provider.validate(session)
        guard node.id == nodeId, node.libraryId == target.library,
            node.kind == (target.operation == "CREATE_FILE" ? .directory : .file),
            node.state == .active, node.trashedAt == nil, node.restoreDeadline == nil,
            !node.purgeEligible
        else { throw UploadFailure.invalidTarget }
        if verifyRevision, case .replaceContent(_, _, let revision) = target,
            node.revision.rawValue != revision.rawValue
        {
            throw UploadFailure.revisionConflict
        }
        if case .createFile(_, _, let name) = target,
            !(try await bridge.validateLogicalName(name))
        {
            throw UploadFailure.invalidMetadata
        }
        return scope
    }

    func create(
        _ intent: UploadIntent, session: LibraryRequestScope, expectedOffset: UInt64?
    ) async throws
        -> UploadSessionEvidence
    {
        let response = try await provider.createUploadSession(intent: intent, session: session)
        guard response.statusCode == 201 else {
            throw mapFailure(response, session: session)
        }
        return try await UploadSessionResponseDecoder(bridge: bridge).decode(
            response, intent: intent, expectedSessionId: intent.idempotencyKey.rawValue,
            expectedOffset: expectedOffset, requireLocation: true)
    }

    func status(_ record: UploadRecord, session: LibraryRequestScope) async throws
        -> UploadSessionEvidence
    {
        guard let sessionId = record.serverSessionId else {
            throw UploadFailure.invalidMetadata
        }
        let response = try await provider.uploadStatus(sessionId: sessionId, session: session)
        guard response.statusCode == 200 else { throw mapFailure(response, session: session) }
        return try await UploadSessionResponseDecoder(bridge: bridge).decode(
            response, intent: record.intent, expectedSessionId: sessionId)
    }

    func patch(
        _ record: UploadRecord, offset: UInt64, bytes: Data, session: LibraryRequestScope
    ) async throws -> UInt64 {
        guard let sessionId = record.serverSessionId else { throw UploadFailure.invalidMetadata }
        let response = try await provider.patchUploadChunk(
            sessionId: sessionId, offset: offset, bytes: bytes, session: session)
        if response.statusCode != 204 { throw mapFailure(response, session: session) }
        return try UploadSessionResponseDecoder.patchOffset(
            response, prior: offset, chunkBytes: bytes.count,
            expectedBytes: record.intent.expected.byteLength)
    }

    func complete(
        _ record: UploadRecord, targetNodeId: NodeId, session: LibraryRequestScope
    ) async throws -> UploadCompletion {
        guard let sessionId = record.serverSessionId else { throw UploadFailure.invalidMetadata }
        let response = try await provider.completeUpload(sessionId: sessionId, session: session)
        guard response.statusCode == 200 else { throw mapFailure(response, session: session) }
        return try await UploadSessionResponseDecoder(bridge: bridge).decodeCompletion(
            response, sessionId: sessionId, intent: record.intent, targetNodeId: targetNodeId)
    }

    func abort(_ record: UploadRecord, session: LibraryRequestScope) async throws
        -> UploadSessionEvidence
    {
        guard let sessionId = record.serverSessionId else { throw UploadFailure.invalidMetadata }
        let response = try await provider.abortUpload(sessionId: sessionId, session: session)
        guard response.statusCode == 200 else { throw mapFailure(response, session: session) }
        return try await UploadSessionResponseDecoder(bridge: bridge).decode(
            response, intent: record.intent, expectedSessionId: sessionId)
    }

    private func mapFailure(
        _ response: HTTPTransportResponse, session: LibraryRequestScope
    ) -> UploadFailure {
        let failure = UploadSessionResponseDecoder.failure(response)
        if failure == .unauthenticated {
            provider.handle(.authenticationRejected, scope: session)
        } else if failure == .deviceRevoked {
            provider.handle(.deviceRevoked, scope: session)
        }
        return failure
    }
}
