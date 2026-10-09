import Foundation

@MainActor
protocol AuthenticatedClientMutationRequestProviderProtocol {
    func begin() async throws -> LibraryRequestScope
    func validate(_ scope: LibraryRequestScope) async throws
    func submitMutation(
        _ mutation: PreparedClientMutation, scope: LibraryRequestScope,
        onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse
    func handle(_ failure: LibraryFailure, scope: LibraryRequestScope)
}

/// A future durable engine must verify persisted identity/bytes, authoritative sync base and
/// recovery ownership before each attempt. P034 supplies no production implementation.
@MainActor
protocol ClientMutationPreparationAuthorizerProtocol {
    func authorizePersistedSubmission(_ mutation: PreparedClientMutation) async throws
}

@MainActor
final class AuthenticatedClientMutationRepository: ClientMutationRepositoryProtocol {
    private let provider: any AuthenticatedClientMutationRequestProviderProtocol
    private let decoder: ClientMutationResponseDecoder
    private let authorizer: (any ClientMutationPreparationAuthorizerProtocol)?

    init(
        provider: any AuthenticatedClientMutationRequestProviderProtocol,
        bridge: any RustBridgeProtocol,
        authorizer: (any ClientMutationPreparationAuthorizerProtocol)? = nil
    ) {
        self.provider = provider
        decoder = ClientMutationResponseDecoder(bridge: bridge)
        self.authorizer = authorizer
    }

    func submit(_ mutation: PreparedClientMutation) async -> ClientMutationSubmissionResult {
        var capturedScope: LibraryRequestScope?
        var dispatched = false
        do {
            try Task.checkCancellation()
            try ClientMutationPolicy.validateRequestSize(mutation.requestBody)
            guard let authorizer else { return .failed(.preparationRequired) }
            let scope = try await provider.begin()
            capturedScope = scope
            guard scope.matches(mutation.base.scope) else { return .failed(.scopeMismatch) }
            try await authorizer.authorizePersistedSubmission(mutation)
            try await provider.validate(scope)
            try Task.checkCancellation()
            let response = try await provider.submitMutation(mutation, scope: scope) {
                dispatched = true
            }
            let result = try await decoder.decode(response, for: mutation)
            // Rust, HTTP and Keychain all suspend. Fence every terminal result, including recovery.
            try await provider.validate(scope)
            try Task.checkCancellation()
            if case .failed(.authenticationRejected) = result {
                provider.handle(.authenticationRejected, scope: scope)
            } else if case .failed(.deviceRevoked) = result {
                provider.handle(.deviceRevoked, scope: scope)
            }
            return result
        } catch {
            var failure = Self.classify(error)
            // Catch paths also fence changed credentials and lifecycle; never trigger recovery here.
            if let scope = capturedScope, !Task.isCancelled {
                do { try await provider.validate(scope) } catch { failure = Self.classify(error) }
            }
            if Task.isCancelled { failure = .cancelled }
            return dispatched ? .outcomeUnknown(failure) : .failed(failure)
        }
    }

    private static func classify(_ error: any Error) -> ClientMutationFailure {
        if let failure = error as? ClientMutationFailure { return failure }
        if error is CancellationError { return .cancelled }
        switch AuthenticatedLibraryCatalogRepository.classify(error) {
        case .unauthenticated: return .unauthenticated
        case .credentialUnavailable: return .credentialUnavailable
        case .invalidCredential: return .invalidCredential
        case .originMismatch: return .originMismatch
        case .staleSession: return .staleSession
        case .authenticationRejected: return .authenticationRejected
        case .deviceRevoked: return .deviceRevoked
        case .cancelled: return .cancelled
        case .offline: return .offline
        case .dnsFailure: return .dnsFailure
        case .timeout: return .timeout
        case .tlsFailure: return .tlsFailure
        case .redirectRejected: return .redirectRejected
        case .resourceLimit: return .resourceLimit
        default: return .protocolFailure
        }
    }
}
