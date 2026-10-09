import Foundation

/// Fetches a complete, transient catalog. Partial data never escapes as a loaded result.
@MainActor
public final class AuthenticatedLibraryCatalogRepository: LibraryCatalogRepositoryProtocol {
    private let provider: any AuthenticatedLibraryRequestProviderProtocol
    private let decoder: LibraryResponseDecoder

    init(provider: any AuthenticatedLibraryRequestProviderProtocol, bridge: any RustBridgeProtocol)
    {
        self.provider = provider
        decoder = LibraryResponseDecoder(bridge: bridge)
    }

    public func listLibraries() async -> LibraryRepositoryResult {
        var scope: LibraryRequestScope?
        do {
            try Task.checkCancellation()
            let current = try await provider.begin()
            scope = current
            var cursor: String?
            var seenCursors: Set<Data> = []
            var seenIds: Set<LibraryId> = []
            var libraries: [Library] = []
            for _ in 0..<LibraryCatalogPolicy.maximumPages {
                try Task.checkCancellation()
                let response = try await provider.requestPage(cursor: cursor, scope: current)
                try Self.validateHTTP(response)
                let page = try await decoder.decode(response.body)
                // Rust validation suspends too; recheck Keychain identity and lifecycle afterward.
                try await provider.validate(current)
                guard
                    page.libraries.count <= LibraryCatalogPolicy.maximumLibraries - libraries.count
                else {
                    throw LibraryFailure.resourceLimit
                }
                for library in page.libraries {
                    guard seenIds.insert(library.id).inserted else {
                        throw LibraryFailure.protocolFailure
                    }
                }
                libraries.append(contentsOf: page.libraries)
                if !page.hasMore {
                    try Task.checkCancellation()
                    return .loaded(libraries)
                }
                guard let next = page.nextCursor else { throw LibraryFailure.protocolFailure }
                guard seenCursors.insert(Data(next.utf8)).inserted else {
                    throw LibraryFailure.repeatedCursor
                }
                cursor = next
            }
            throw LibraryFailure.resourceLimit
        } catch {
            let failure = Task.isCancelled ? LibraryFailure.cancelled : Self.classify(error)
            if let scope { provider.handle(failure, scope: scope) }
            return .failed(failure)
        }
    }

    static func validateHTTP(_ response: HTTPTransportResponse) throws {
        guard response.body.count <= LibraryCatalogPolicy.maximumResponseBytes else {
            throw LibraryFailure.resourceLimit
        }
        if (300...399).contains(response.statusCode) { throw LibraryFailure.redirectRejected }
        let contentType = response.headers.first { $0.key.lowercased() == "content-type" }?.value
        guard
            contentType?.components(separatedBy: ";").first?
                .trimmingCharacters(in: .whitespacesAndNewlines).lowercased() == "application/json"
        else { throw LibraryFailure.unexpectedContentType }
        guard response.statusCode != 200 else { return }
        let code = try LibraryResponseDecoder.errorCode(response.body)
        if response.statusCode == 401 {
            switch code {
            case "authentication_failed": throw LibraryFailure.authenticationRejected
            case "device_revoked": throw LibraryFailure.deviceRevoked
            default: throw LibraryFailure.protocolFailure
            }
        }
        if response.statusCode == 503 { throw LibraryFailure.serverUnavailable }
        throw LibraryFailure.httpFailure(statusCode: response.statusCode)
    }

    static func classify(_ error: any Error) -> LibraryFailure {
        if let failure = error as? LibraryFailure { return failure }
        if error is CancellationError { return .cancelled }
        guard let transport = error as? SynveilTransportError else { return .offline }
        switch transport {
        case .offline: return .offline
        case .dnsFailure: return .dnsFailure
        case .timeout: return .timeout
        case .tlsError: return .tlsFailure
        case .redirectRejected: return .redirectRejected
        case .cancelled: return .cancelled
        case .bodyLimitExceeded: return .resourceLimit
        case .unexpectedContentType: return .unexpectedContentType
        case .httpError, .malformedResponse, .protocolError, .configurationError:
            return .protocolFailure
        }
    }
}
