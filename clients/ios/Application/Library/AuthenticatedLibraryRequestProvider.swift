import Foundation

/// Transient operation identity. Never published through observable state or cached persistently.
struct LibraryRequestScope: Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    fileprivate let session: DeviceCredentialSession
    fileprivate let revision: UInt64

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

/// Node listing reuses the catalog's session acquisition, identity checks and recovery handler.
@MainActor
protocol AuthenticatedNodeRequestProviderProtocol {
    func begin() async throws -> LibraryRequestScope
    func requestChildrenPage(
        libraryId: LibraryId, parent: NodeParentScope, cursor: String?, scope: LibraryRequestScope
    ) async throws -> HTTPTransportResponse
    func validate(_ scope: LibraryRequestScope) async throws
    func handle(_ failure: NodeFailure, scope: LibraryRequestScope)
}

@MainActor
final class AuthenticatedLibraryRequestProvider: AuthenticatedLibraryRequestProviderProtocol,
    AuthenticatedNodeRequestProviderProtocol
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
        try await validate(scope)
        let endpoint = scope.session.serverEndpoint
        guard var components = URLComponents(url: endpoint.url, resolvingAgainstBaseURL: false)
        else {
            throw LibraryFailure.originMismatch
        }
        let path = components.percentEncodedPath.trimmingCharacters(
            in: CharacterSet(charactersIn: "/"))
        components.percentEncodedPath = (path.isEmpty ? "" : "/\(path)") + collectionPath
        components.queryItems = [
            URLQueryItem(name: "limit", value: String(LibraryCatalogPolicy.pageSize))
        ]
        if let parentId {
            components.queryItems?.append(URLQueryItem(name: "parent_id", value: parentId.rawValue))
        }
        if let cursor { components.queryItems?.append(URLQueryItem(name: "cursor", value: cursor)) }
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
            headers: [
                "Authorization": "Bearer \(scope.session.record.credential.rawValue)",
                "Accept": "application/json",
                "Accept-Encoding": "identity",
                "User-Agent": "Synveil/0.1.0 (iOS)",
            ])
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
