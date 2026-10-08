import Foundation

/// Remote authorization outcome after Keychain loading has already validated the session locally.
public enum SessionRestorationVerificationFailure: Equatable, Sendable {
    case authenticationRejected
    case deviceRevoked
    case serverUnavailable
    case dnsFailure
    case networkFailure
    case timeout
    case tlsFailure
    case unexpectedContentType
    case unexpectedProtocolResponse
    case redirectRejected
    case cancelled
}

/// Typed result of looking up and remotely checking the single stored device session.
///
/// Associated sessions are transient in-memory values. `SessionController` keeps them private;
/// `AppStartupState` and SwiftUI navigation never contain credentials.
public enum SessionRestorationResult: Equatable, Sendable {
    case noStoredSession(configuredServerEndpoint: ServerEndpoint?)
    case locallyValidStoredSession(
        DeviceCredentialSession,
        verification: SessionRestorationVerificationFailure
    )
    case remotelyVerifiedAuthorizedSession(DeviceCredentialSession)
    case keychainUnavailable
    case keychainReadFailure
    case corruptedKeychainSession
    case unsupportedKeychainFormat(Int)
    case serverOriginScopeMismatch
    case sessionChanged(ServerEndpoint)
    case invalidCredential
    case keychainFailure
    case cancelled
}

/// Testable orchestration boundary for startup session restoration and retry.
public protocol SessionRestorationServiceProtocol: Sendable {
    func restore(configuredServerEndpoint: ServerEndpoint?) async -> SessionRestorationResult

    func retryVerification(
        of session: DeviceCredentialSession,
        expectedServerEndpoint: ServerEndpoint
    ) async -> SessionRestorationResult
}

/// Restores the one Keychain session and verifies it with an authenticated read-only API operation.
public final class SessionRestorationService: SessionRestorationServiceProtocol, Sendable {
    private let credentialStore: SecureCredentialSinkProtocol
    private let authorizationValidator: AuthenticatedSessionValidationProtocol

    public init(
        credentialStore: SecureCredentialSinkProtocol,
        authorizationValidator: AuthenticatedSessionValidationProtocol
    ) {
        self.credentialStore = credentialStore
        self.authorizationValidator = authorizationValidator
    }

    public func restore(
        configuredServerEndpoint: ServerEndpoint?
    ) async -> SessionRestorationResult {
        let session: DeviceCredentialSession
        do {
            session = try await credentialStore.load(
                expectedServerEndpoint: configuredServerEndpoint
            )
        } catch is CancellationError {
            return .cancelled
        } catch let error as SecureCredentialSinkError {
            return Task.isCancelled
                ? .cancelled
                : mapStorageError(error, configuredEndpoint: configuredServerEndpoint)
        } catch {
            return Task.isCancelled ? .cancelled : .keychainFailure
        }

        if Task.isCancelled {
            return .cancelled
        }
        guard
            configuredServerEndpoint == nil
                || configuredServerEndpoint == session.serverEndpoint
        else {
            return .serverOriginScopeMismatch
        }

        return await verify(session)
    }

    public func retryVerification(
        of session: DeviceCredentialSession,
        expectedServerEndpoint: ServerEndpoint
    ) async -> SessionRestorationResult {
        guard session.serverEndpoint == expectedServerEndpoint else {
            return .serverOriginScopeMismatch
        }
        if Task.isCancelled {
            return .locallyValidStoredSession(session, verification: .cancelled)
        }

        let currentSession: DeviceCredentialSession
        do {
            currentSession = try await credentialStore.load(
                expectedServerEndpoint: expectedServerEndpoint
            )
        } catch is CancellationError {
            return .locallyValidStoredSession(session, verification: .cancelled)
        } catch let error as SecureCredentialSinkError {
            if Task.isCancelled {
                return .locallyValidStoredSession(session, verification: .cancelled)
            }
            switch error {
            case .itemNotFound:
                return .sessionChanged(expectedServerEndpoint)
            default:
                return mapStorageError(error, configuredEndpoint: expectedServerEndpoint)
            }
        } catch {
            return Task.isCancelled
                ? .locallyValidStoredSession(session, verification: .cancelled)
                : .keychainFailure
        }

        guard !Task.isCancelled else {
            return .locallyValidStoredSession(session, verification: .cancelled)
        }
        guard currentSession == session else {
            return .sessionChanged(expectedServerEndpoint)
        }
        return await verify(currentSession)
    }

    private func verify(_ session: DeviceCredentialSession) async -> SessionRestorationResult {
        if Task.isCancelled {
            return .locallyValidStoredSession(session, verification: .cancelled)
        }

        let validation = await authorizationValidator.validateAuthorization(for: session)
        if Task.isCancelled {
            return .locallyValidStoredSession(session, verification: .cancelled)
        }

        return await confirmSessionIdentityIsCurrent(session, validation: validation)
    }

    private func confirmSessionIdentityIsCurrent(
        _ verifiedSession: DeviceCredentialSession,
        validation: AuthenticatedSessionValidationResult
    ) async -> SessionRestorationResult {
        if Task.isCancelled {
            return .locallyValidStoredSession(verifiedSession, verification: .cancelled)
        }

        let currentSession: DeviceCredentialSession
        do {
            currentSession = try await credentialStore.load(
                expectedServerEndpoint: verifiedSession.serverEndpoint
            )
        } catch is CancellationError {
            return .locallyValidStoredSession(verifiedSession, verification: .cancelled)
        } catch let error as SecureCredentialSinkError {
            if Task.isCancelled {
                return .locallyValidStoredSession(verifiedSession, verification: .cancelled)
            }
            switch error {
            case .itemNotFound:
                return .sessionChanged(verifiedSession.serverEndpoint)
            default:
                return mapStorageError(error, configuredEndpoint: verifiedSession.serverEndpoint)
            }
        } catch {
            return Task.isCancelled
                ? .locallyValidStoredSession(verifiedSession, verification: .cancelled)
                : .keychainFailure
        }

        guard !Task.isCancelled else {
            return .locallyValidStoredSession(verifiedSession, verification: .cancelled)
        }
        guard currentSession == verifiedSession else {
            return .sessionChanged(verifiedSession.serverEndpoint)
        }

        switch validation {
        case .authorized:
            return .remotelyVerifiedAuthorizedSession(verifiedSession)
        case .authenticationRejected:
            return .locallyValidStoredSession(
                verifiedSession,
                verification: .authenticationRejected
            )
        case .deviceRevoked:
            return .locallyValidStoredSession(verifiedSession, verification: .deviceRevoked)
        case .serverUnavailable:
            return .locallyValidStoredSession(verifiedSession, verification: .serverUnavailable)
        case .dnsFailure:
            return .locallyValidStoredSession(verifiedSession, verification: .dnsFailure)
        case .networkFailure:
            return .locallyValidStoredSession(verifiedSession, verification: .networkFailure)
        case .timeout:
            return .locallyValidStoredSession(verifiedSession, verification: .timeout)
        case .tlsFailure:
            return .locallyValidStoredSession(verifiedSession, verification: .tlsFailure)
        case .unexpectedContentType:
            return .locallyValidStoredSession(verifiedSession, verification: .unexpectedContentType)
        case .unexpectedProtocolResponse:
            return .locallyValidStoredSession(
                verifiedSession,
                verification: .unexpectedProtocolResponse
            )
        case .redirectRejected:
            return .locallyValidStoredSession(verifiedSession, verification: .redirectRejected)
        case .cancelled:
            return .locallyValidStoredSession(verifiedSession, verification: .cancelled)
        }
    }

    private func mapStorageError(
        _ error: SecureCredentialSinkError,
        configuredEndpoint: ServerEndpoint?
    ) -> SessionRestorationResult {
        switch error {
        case .itemNotFound:
            return .noStoredSession(configuredServerEndpoint: configuredEndpoint)
        case .unavailable:
            return .keychainUnavailable
        case .corruptPayload, .verificationFailure:
            return .corruptedKeychainSession
        case .unsupportedFormat(let version):
            return .unsupportedKeychainFormat(version)
        case .scopeMismatch:
            return .serverOriginScopeMismatch
        case .invalidCredential:
            return .invalidCredential
        case .readFailure:
            return .keychainReadFailure
        case .duplicateItem, .writeFailure, .deletionFailure, .unexpectedOSStatus:
            return Task.isCancelled ? .cancelled : .keychainFailure
        }
    }
}
