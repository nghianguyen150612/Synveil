import Foundation

/// Transient operation identity. Never published through observable state or cached persistently.
struct LibraryRequestScope: Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    fileprivate let session: DeviceCredentialSession
    fileprivate let revision: UInt64

    init(session: DeviceCredentialSession, revision: UInt64) {
        self.session = session
        self.revision = revision
    }

    var serverEndpoint: ServerEndpoint { session.serverEndpoint }
    var ownerUserId: String { session.record.ownerUserId }
    var deviceIdentifier: String { session.record.deviceId }

    func matches(_ scope: ClientMutationScope) -> Bool {
        session.serverEndpoint == scope.serverEndpoint
            && session.record.ownerUserId == scope.ownerUserId
            && session.record.deviceId == scope.deviceId.rawValue
    }

    var credentialIdentifier: String { session.record.credentialId }
    var lifecycleRevision: UInt64 { revision }

    func sameSession(as other: LibraryRequestScope) -> Bool {
        revision == other.revision && session == other.session
    }

    func mutationScope(libraryId: LibraryId, bridge: any RustBridgeProtocol) async throws
        -> ClientMutationScope
    {
        let device = try await ClientMutationDeviceId.validated(
            session.record.deviceId, using: bridge)
        _ = try await LibraryId.validated(libraryId.rawValue, using: bridge)
        guard try await bridge.validateNodeID(session.record.ownerUserId) else {
            throw LibraryFailure.invalidCredential
        }
        return ClientMutationScope(
            serverEndpoint: session.serverEndpoint, ownerUserId: session.record.ownerUserId,
            deviceId: device, libraryId: libraryId)
    }

    var description: String { "[REDACTED_LIBRARY_REQUEST_SCOPE]" }
    var debugDescription: String { description }
}

/// Internal security boundary: the repository supplies opaque cursors, never URLs or headers.
@MainActor
protocol LocalLibrarySessionProviderProtocol {
    func begin() async throws -> LibraryRequestScope
    func validate(_ scope: LibraryRequestScope) async throws
}

@MainActor
protocol AuthenticatedLibraryRequestProviderProtocol: LocalLibrarySessionProviderProtocol {
    func requestPage(cursor: String?, scope: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    func handle(_ failure: LibraryFailure, scope: LibraryRequestScope)
}

/// Node reads reuse the catalog's session acquisition, identity checks and recovery handler.
@MainActor
protocol AuthenticatedNodeRequestProviderProtocol {
    func begin() async throws -> LibraryRequestScope
    func requestNode(nodeId: NodeId, scope: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    func requestChildrenPage(
        libraryId: LibraryId, parent: NodeParentScope, cursor: String?, scope: LibraryRequestScope
    ) async throws -> HTTPTransportResponse
    func validate(_ scope: LibraryRequestScope) async throws
    func handle(_ failure: NodeFailure, scope: LibraryRequestScope)
}

@MainActor
protocol AuthenticatedUploadRequestProviderProtocol: AuthenticatedLibraryRequestProviderProtocol,
    AuthenticatedNodeRequestProviderProtocol
{
    func createUploadSession(
        intent: UploadIntent, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse
    func uploadStatus(
        sessionId: String, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse
    func patchUploadChunk(
        sessionId: String, offset: UInt64, bytes: Data, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse
    func backgroundUploadRequest(
        record: UploadRecord, chunk: BackgroundChunkIntent,
        session: LibraryRequestScope
    ) async throws -> HTTPTransportRequest
    func completeUpload(
        sessionId: String, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse
    func abortUpload(
        sessionId: String, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse
}

extension AuthenticatedUploadRequestProviderProtocol {
    func backgroundUploadRequest(
        record: UploadRecord, chunk: BackgroundChunkIntent,
        session: LibraryRequestScope
    ) async throws -> HTTPTransportRequest {
        throw UploadFailure.scopeMismatch
    }
}

@MainActor
final class AuthenticatedLibraryRequestProvider: AuthenticatedLibraryRequestProviderProtocol,
    AuthenticatedNodeRequestProviderProtocol, AuthenticatedClientMutationRequestProviderProtocol,
    AuthenticatedSyncCheckpointRequestProviderProtocol,
    AuthenticatedSyncFeedRequestProviderProtocol,
    AuthenticatedRebaselineRequestProviderProtocol,
    AuthenticatedFileDownloadRequestProviderProtocol,
    AuthenticatedUploadRequestProviderProtocol
{
    private let controller: SessionController
    private let store: any SecureCredentialSinkProtocol
    private let transport: any HTTPTransportProtocol
    private let contentTransport: any FileContentTransportProtocol
    private let uploadTransport: any RawUploadTransportProtocol

    init(
        controller: SessionController, store: any SecureCredentialSinkProtocol,
        transport: any HTTPTransportProtocol,
        contentTransport: any FileContentTransportProtocol = URLSessionFileContentTransport(),
        uploadTransport: any RawUploadTransportProtocol = URLSessionRawUploadTransport()
    ) {
        self.controller = controller
        self.store = store
        self.transport = transport
        self.contentTransport = contentTransport
        self.uploadTransport = uploadTransport
    }

    func begin() async throws -> LibraryRequestScope {
        try Task.checkCancellation()
        guard controller.state == .authenticated, let endpoint = controller.serverEndpoint else {
            throw LibraryFailure.unauthenticated
        }
        guard endpoint.isSecureScheme else { throw LibraryFailure.originMismatch }
        let revision = controller.lifecycleRevision
        let session = try await load(endpoint)
        let scope = LibraryRequestScope(session: session, revision: revision)
        try checkCurrent(scope)
        return scope
    }

    func validate(_ scope: LibraryRequestScope) async throws {
        try checkCurrent(scope)
        let current = try await load(scope.session.serverEndpoint)
        try checkCurrent(scope)
        guard current == scope.session else { throw LibraryFailure.staleSession }
    }

    func requestPage(cursor: String?, scope: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    {
        try await requestCollectionPage(
            path: "/api/v1/libraries", parentId: nil, cursor: cursor, scope: scope)
    }

    func requestChildrenPage(
        libraryId: LibraryId, parent: NodeParentScope, cursor: String?, scope: LibraryRequestScope
    ) async throws -> HTTPTransportResponse {
        try await requestCollectionPage(
            path: "/api/v1/libraries/\(libraryId.rawValue)/nodes",
            parentId: parent.queryParentId, cursor: cursor, scope: scope)
    }

    private func requestCollectionPage(
        path collectionPath: String, parentId: NodeId?, cursor: String?, scope: LibraryRequestScope
    ) async throws -> HTTPTransportResponse {
        if let cursor, !LibraryWireValidation.validCursor(cursor) {
            throw LibraryFailure.protocolFailure
        }
        var queryItems = [URLQueryItem(name: "limit", value: String(LibraryCatalogPolicy.pageSize))]
        if let parentId {
            queryItems.append(URLQueryItem(name: "parent_id", value: parentId.rawValue))
        }
        if let cursor { queryItems.append(URLQueryItem(name: "cursor", value: cursor)) }
        return try await requestGET(path: collectionPath, queryItems: queryItems, scope: scope)
    }

    func requestNode(nodeId: NodeId, scope: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    {
        try await requestGET(
            path: "/api/v1/nodes/\(nodeId.rawValue)", queryItems: nil, scope: scope)
    }

    func requestCheckpoint(scope mutationScope: ClientMutationScope, session: LibraryRequestScope)
        async throws -> HTTPTransportResponse
    {
        guard session.matches(mutationScope) else { throw MutationQueueFailure.scopeMismatch }
        return try await requestGET(
            path:
                "/api/v1/devices/\(session.session.record.deviceId)/libraries/\(mutationScope.libraryId.rawValue)/checkpoint",
            queryItems: nil, scope: session)
    }

    func requestFeed(scope: ClientMutationScope, session: LibraryRequestScope, limit: Int)
        async throws -> HTTPTransportResponse
    {
        guard (1...500).contains(limit) else { throw SyncFeedFailure.protocolFailure }
        guard session.matches(scope) else { throw SyncFeedFailure.scopeMismatch }
        return try await requestGET(
            path:
                "/api/v1/devices/\(scope.deviceId.rawValue)/libraries/\(scope.libraryId.rawValue)/changes",
            queryItems: [URLQueryItem(name: "limit", value: String(limit))], scope: session)
    }

    func submitSyncAck(
        _ receipt: AppliedFeedCommitReceipt, session: LibraryRequestScope, onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse {
        guard receipt.matches(session) else { throw SyncFeedFailure.staleSession }
        let evidence = receipt.evidence
        let body = try SyncFeedPolicy.ackBody(evidence)
        guard session.matches(evidence.scope) else { throw SyncFeedFailure.scopeMismatch }
        try await validate(session)
        let endpoint = session.session.serverEndpoint
        guard var components = URLComponents(url: endpoint.url, resolvingAgainstBaseURL: false),
            components.user == nil, components.password == nil
        else { throw LibraryFailure.originMismatch }
        let basePath = components.percentEncodedPath.trimmingCharacters(
            in: CharacterSet(charactersIn: "/"))
        components.percentEncodedPath =
            (basePath.isEmpty ? "" : "/\(basePath)")
            + "/api/v1/devices/\(evidence.scope.deviceId.rawValue)/libraries/\(evidence.scope.libraryId.rawValue)/changes/ack"
        components.query = nil
        components.fragment = nil
        guard let url = components.url, url.scheme == "https", url.host == endpoint.host,
            url.port == endpoint.port
        else { throw LibraryFailure.originMismatch }
        try checkCurrent(session)
        let request = HTTPTransportRequest(
            url: url, method: .post,
            headers: authenticatedHeaders(session, jsonBody: true), body: body)
        onDispatch()
        let response = try await transport.send(request)
        try await validate(session)
        return response
    }

    func requestVersion(version: FileVersionId, session: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    {
        try await requestGET(
            path: "/api/v1/versions/\(version.rawValue)", queryItems: nil, scope: session)
    }

    func requestContent(
        identity: DownloadIdentity, session: LibraryRequestScope, range: DownloadRange?,
        revalidating: Bool = false, receive: @escaping @Sendable (Data) async throws -> Void
    ) async throws {
        guard session.matches(identity.scope), session.credentialIdentifier == identity.credentialId
        else {
            throw DownloadFailure.scopeMismatch
        }
        try await validate(session)
        let endpoint = identity.scope.serverEndpoint
        guard var components = URLComponents(url: endpoint.url, resolvingAgainstBaseURL: false)
        else {
            throw DownloadFailure.scopeMismatch
        }
        let base = components.percentEncodedPath.trimmingCharacters(
            in: CharacterSet(charactersIn: "/"))
        components.percentEncodedPath =
            (base.isEmpty ? "" : "/\(base)")
            + "/api/v1/versions/\(identity.versionId.rawValue)/content"
        components.query = nil
        components.fragment = nil
        guard let url = components.url, url.scheme == "https", url.host == endpoint.host,
            url.port == endpoint.port, components.user == nil, components.password == nil
        else { throw DownloadFailure.scopeMismatch }
        var headers = authenticatedHeaders(session)
        headers["Accept"] = "application/octet-stream"
        headers["Range"] = range?.header
        headers["If-None-Match"] = revalidating ? identity.expected.etag : nil
        try checkCurrent(session)
        do {
            try await contentTransport.stream(
                HTTPTransportRequest(url: url, headers: headers), expected: identity.expected,
                range: range, revalidating: revalidating, receive: receive)
            try await validate(session)
        } catch {
            try checkCurrent(session)
            let failure = DownloadPolicy.classify(error)
            if failure == .unauthenticated { handle(.authenticationRejected, scope: session) }
            if failure == .deviceRevoked { handle(.deviceRevoked, scope: session) }
            throw failure
        }
    }

    func createUploadSession(
        intent: UploadIntent, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse {
        guard intent.requestBytes.count <= UploadPolicy.maximumJSONBytes else {
            throw UploadFailure.payloadTooLarge
        }
        return try await uploadRequest(
            method: .post, path: "/api/v1/upload-sessions", body: intent.requestBytes,
            session: session, json: true)
    }

    func uploadStatus(
        sessionId: String, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse {
        try await uploadRequest(
            method: .get, path: try uploadSessionPath(sessionId), body: nil,
            session: session, json: false)
    }

    func patchUploadChunk(
        sessionId: String, offset: UInt64, bytes: Data, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse {
        guard !bytes.isEmpty, bytes.count <= UploadPolicy.chunkBytes else {
            throw UploadFailure.chunkTooLarge
        }
        var headers = authenticatedHeaders(session)
        headers["Content-Type"] = "application/octet-stream"
        headers["Upload-Offset"] = String(offset)
        headers["Cache-Control"] = "no-store"
        return try await uploadRequest(
            method: .patch, path: try uploadSessionPath(sessionId), body: bytes,
            session: session, headers: headers)
    }

    func backgroundUploadRequest(
        record: UploadRecord, chunk: BackgroundChunkIntent,
        session: LibraryRequestScope
    ) async throws -> HTTPTransportRequest {
        try await validate(session)
        guard session.matches(record.scope), session.credentialIdentifier == record.credentialId,
            let id = record.serverSessionId, id == record.intent.idempotencyKey.rawValue,
            chunk.offset == record.authoritativeOffset, !record.uncertain,
            chunk.length > 0, chunk.length <= UploadPolicy.chunkBytes,
            chunk.offset <= record.intent.expected.byteLength,
            UInt64(chunk.length) <= record.intent.expected.byteLength - chunk.offset
        else { throw UploadFailure.scopeMismatch }
        let endpoint = session.session.serverEndpoint
        guard var components = URLComponents(url: endpoint.url, resolvingAgainstBaseURL: false)
        else {
            throw UploadFailure.scopeMismatch
        }
        let base = components.percentEncodedPath.trimmingCharacters(
            in: CharacterSet(charactersIn: "/"))
        components.percentEncodedPath =
            (base.isEmpty ? "" : "/\(base)") + (try uploadSessionPath(id))
        components.query = nil
        components.fragment = nil
        guard let url = components.url, url.scheme == "https", url.host == endpoint.host,
            url.port == endpoint.port, url.user == nil, url.password == nil
        else { throw UploadFailure.scopeMismatch }
        var headers = authenticatedHeaders(session)
        headers["Content-Type"] = "application/octet-stream"
        headers["Upload-Offset"] = String(chunk.offset)
        headers["Content-Length"] = String(chunk.length)
        headers["Cache-Control"] = "no-store"
        try checkCurrent(session)
        return HTTPTransportRequest(url: url, method: .patch, headers: headers)
    }

    func completeUpload(
        sessionId: String, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse {
        try await uploadRequest(
            method: .post, path: try uploadSessionPath(sessionId) + "/complete", body: nil,
            session: session, json: false)
    }

    func abortUpload(
        sessionId: String, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse {
        try await uploadRequest(
            method: .post, path: try uploadSessionPath(sessionId) + "/abort", body: nil,
            session: session, json: false)
    }

    private func uploadSessionPath(_ id: String) throws -> String {
        guard UUID(uuidString: id)?.uuidString.lowercased() == id else {
            throw UploadFailure.invalidMetadata
        }
        return "/api/v1/upload-sessions/\(id)"
    }

    private func uploadRequest(
        method: HTTPMethod, path resourcePath: String, body: Data?,
        session: LibraryRequestScope, json: Bool = false,
        headers explicitHeaders: [String: String]? = nil
    ) async throws -> HTTPTransportResponse {
        try await validate(session)
        let endpoint = session.session.serverEndpoint
        guard var components = URLComponents(url: endpoint.url, resolvingAgainstBaseURL: false),
            components.user == nil, components.password == nil
        else { throw UploadFailure.scopeMismatch }
        let base = components.percentEncodedPath.trimmingCharacters(
            in: CharacterSet(charactersIn: "/"))
        components.percentEncodedPath = (base.isEmpty ? "" : "/\(base)") + resourcePath
        components.query = nil
        components.fragment = nil
        guard let url = components.url, url.scheme == "https", url.host == endpoint.host,
            url.port == endpoint.port
        else { throw UploadFailure.scopeMismatch }
        var headers = explicitHeaders ?? authenticatedHeaders(session, jsonBody: json)
        headers["Cache-Control"] = "no-store"
        headers["Cookie"] = nil
        headers["X-CSRF-Token"] = nil
        try checkCurrent(session)
        do {
            let response = try await uploadTransport.send(
                HTTPTransportRequest(url: url, method: method, headers: headers, body: body))
            try await validate(session)
            return response
        } catch let failure as UploadFailure {
            try checkCurrent(session)
            throw failure
        } catch {
            try checkCurrent(session)
            throw UploadPolicy.classify(error)
        }
    }

    private func requestGET(
        path resourcePath: String, queryItems: [URLQueryItem]?, scope: LibraryRequestScope
    ) async throws -> HTTPTransportResponse {
        try await validate(scope)
        let endpoint = scope.session.serverEndpoint
        guard var components = URLComponents(url: endpoint.url, resolvingAgainstBaseURL: false)
        else { throw LibraryFailure.originMismatch }
        let path = components.percentEncodedPath.trimmingCharacters(
            in: CharacterSet(charactersIn: "/"))
        components.percentEncodedPath = (path.isEmpty ? "" : "/\(path)") + resourcePath
        components.queryItems = queryItems
        // Form-style server query parsers treat a literal plus as a space.
        components.percentEncodedQuery = components.percentEncodedQuery?.replacingOccurrences(
            of: "+", with: "%2B")
        components.fragment = nil
        guard let url = components.url, url.scheme == "https", url.host == endpoint.host,
            url.port == endpoint.port
        else {
            throw LibraryFailure.originMismatch
        }
        try checkCurrent(scope)
        let request = HTTPTransportRequest(
            url: url,
            headers: authenticatedHeaders(scope))
        do {
            let response = try await transport.send(request)
            try await validate(scope)
            return response
        } catch {
            // Cancellation/logout/origin changes take precedence over late HTTP or network failures.
            try checkCurrent(scope)
            throw error
        }
    }

    /// One attempt only. The dispatch marker is set after all local/session checks, immediately
    /// before invoking transport; any later inability to verify the result is ambiguous.
    func submitMutation(
        _ mutation: PreparedClientMutation, scope: LibraryRequestScope,
        onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse {
        try ClientMutationPolicy.validateRequestSize(mutation.requestBody)
        guard scope.matches(mutation.base.scope) else { throw ClientMutationFailure.scopeMismatch }
        try await validate(scope)
        let endpoint = scope.session.serverEndpoint
        guard var components = URLComponents(url: endpoint.url, resolvingAgainstBaseURL: false),
            components.user == nil, components.password == nil
        else { throw LibraryFailure.originMismatch }
        let basePath = components.percentEncodedPath.trimmingCharacters(
            in: CharacterSet(charactersIn: "/"))
        components.percentEncodedPath =
            (basePath.isEmpty ? "" : "/\(basePath)")
            + "/api/v1/devices/\(scope.session.record.deviceId)/libraries/\(mutation.base.scope.libraryId.rawValue)/mutations"
        components.query = nil
        components.fragment = nil
        guard let url = components.url, url.scheme == "https", url.host == endpoint.host,
            url.port == endpoint.port
        else { throw LibraryFailure.originMismatch }
        try checkCurrent(scope)
        let request = HTTPTransportRequest(
            url: url, method: .post,
            headers: authenticatedHeaders(scope, jsonBody: true), body: mutation.requestBody)
        onDispatch()
        let response = try await transport.send(request)
        try await validate(scope)
        return response
    }

    func startRebaseline(
        scope: ClientMutationScope, session: LibraryRequestScope, onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse {
        guard session.matches(scope) else { throw RebaselineFailure.scopeMismatch }
        return try await rebaselinePOST(
            scope: scope, suffix: "", body: Data("{}".utf8), session: session,
            onDispatch: onDispatch)
    }

    func requestRebaselinePage(
        bootstrap: RebaselineBootstrap, cursor: String?, limit: Int, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse {
        guard session.matches(bootstrap.scope), (1...1000).contains(limit),
            cursor.map({ RebaselinePolicy.validOpaque($0, maximum: 320) }) ?? true
        else { throw RebaselineFailure.protocolFailure }
        var query = [URLQueryItem(name: "limit", value: String(limit))]
        if let cursor { query.append(URLQueryItem(name: "cursor", value: cursor)) }
        return try await requestGET(
            path: rebaselinePath(bootstrap.scope) + "/\(bootstrap.id.rawValue)/nodes",
            queryItems: query, scope: session)
    }

    func completeRebaseline(
        _ handoff: RebaselineHandoff, session: LibraryRequestScope, onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse {
        guard session.matches(handoff.bootstrap.scope),
            session.credentialIdentifier == handoff.credentialId
        else { throw RebaselineFailure.staleSession }
        return try await rebaselinePOST(
            scope: handoff.bootstrap.scope, suffix: "/\(handoff.bootstrap.id.rawValue)/complete",
            body: RebaselinePolicy.completionBody(handoff.token), session: session,
            onDispatch: onDispatch)
    }

    private func rebaselinePath(_ scope: ClientMutationScope) -> String {
        "/api/v1/devices/\(scope.deviceId.rawValue)/libraries/\(scope.libraryId.rawValue)/rebaseline"
    }

    private func rebaselinePOST(
        scope: ClientMutationScope, suffix: String, body: Data, session: LibraryRequestScope,
        onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse {
        try await validate(session)
        let endpoint = session.session.serverEndpoint
        guard var components = URLComponents(url: endpoint.url, resolvingAgainstBaseURL: false),
            components.user == nil, components.password == nil
        else { throw LibraryFailure.originMismatch }
        let base = components.percentEncodedPath.trimmingCharacters(
            in: CharacterSet(charactersIn: "/"))
        components.percentEncodedPath =
            (base.isEmpty ? "" : "/\(base)") + rebaselinePath(scope) + suffix
        components.query = nil
        components.fragment = nil
        guard let url = components.url, url.scheme == "https", url.host == endpoint.host,
            url.port == endpoint.port
        else { throw LibraryFailure.originMismatch }
        try checkCurrent(session)
        var headers = authenticatedHeaders(session, jsonBody: true)
        headers["Cache-Control"] = "no-store"
        let request = HTTPTransportRequest(url: url, method: .post, headers: headers, body: body)
        onDispatch()
        let response = try await transport.send(request)
        try await validate(session)
        return response
    }

    private func authenticatedHeaders(_ scope: LibraryRequestScope, jsonBody: Bool = false)
        -> [String: String]
    {
        var headers = [
            "Authorization": "Bearer \(scope.session.record.credential.rawValue)",
            "Accept": "application/json", "Accept-Encoding": "identity",
            "User-Agent": "Synveil/0.1.0 (iOS)",
        ]
        if jsonBody { headers["Content-Type"] = "application/json" }
        return headers
    }

    func handle(_ failure: LibraryFailure, scope: LibraryRequestScope) {
        guard (try? checkCurrent(scope)) != nil else { return }
        controller.handleLibraryAuthenticationFailure(failure, revision: scope.revision)
    }

    private func checkCurrent(_ scope: LibraryRequestScope) throws {
        try Task.checkCancellation()
        guard controller.state == .authenticated,
            controller.lifecycleRevision == scope.revision,
            controller.serverEndpoint == scope.session.serverEndpoint
        else { throw LibraryFailure.staleSession }
    }

    private func load(_ endpoint: ServerEndpoint) async throws -> DeviceCredentialSession {
        do {
            let session = try await store.load(expectedServerEndpoint: endpoint)
            guard session.serverEndpoint == endpoint else { throw LibraryFailure.originMismatch }
            guard DeviceCredential.isValid(session.record.credential.rawValue) else {
                throw LibraryFailure.invalidCredential
            }
            return session
        } catch is CancellationError {
            throw LibraryFailure.cancelled
        } catch let failure as LibraryFailure {
            throw failure
        } catch let error as SecureCredentialSinkError {
            switch error {
            case .scopeMismatch: throw LibraryFailure.originMismatch
            case .invalidCredential: throw LibraryFailure.invalidCredential
            default: throw LibraryFailure.credentialUnavailable
            }
        } catch {
            throw LibraryFailure.credentialUnavailable
        }
    }
}
