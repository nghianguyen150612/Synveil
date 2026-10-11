import Foundation

/// Transient operation identity. Never published through observable state or cached persistently.
struct LibraryRequestScope: Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    fileprivate let session: DeviceCredentialSession
    fileprivate let revision: UInt64

    func matches(_ scope: ClientMutationScope) -> Bool {
        session.serverEndpoint == scope.serverEndpoint
            && session.record.ownerUserId == scope.ownerUserId
            && session.record.deviceId == scope.deviceId.rawValue
    }

    var credentialIdentifier: String { session.record.credentialId }

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
protocol AuthenticatedLibraryRequestProviderProtocol {
    func begin() async throws -> LibraryRequestScope
    func requestPage(cursor: String?, scope: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    func validate(_ scope: LibraryRequestScope) async throws
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
final class AuthenticatedLibraryRequestProvider: AuthenticatedLibraryRequestProviderProtocol,
    AuthenticatedNodeRequestProviderProtocol, AuthenticatedClientMutationRequestProviderProtocol,
    AuthenticatedSyncCheckpointRequestProviderProtocol,
    AuthenticatedSyncFeedRequestProviderProtocol,
    AuthenticatedRebaselineRequestProviderProtocol
{
    private let controller: SessionController
    private let store: any SecureCredentialSinkProtocol
    private let transport: any HTTPTransportProtocol

    init(
        controller: SessionController, store: any SecureCredentialSinkProtocol,
        transport: any HTTPTransportProtocol
    ) {
        self.controller = controller
        self.store = store
        self.transport = transport
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
