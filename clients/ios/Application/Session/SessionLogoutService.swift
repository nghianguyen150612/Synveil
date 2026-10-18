import Foundation

/// Non-secret result of local device-session cleanup.
public enum SessionLogoutResult: Equatable, Sendable {
    /// The active credential item was verified absent from secure storage.
    case credentialAbsent

    /// Secure cleanup did not complete or could not be verified.
    case failed(SessionLogoutError)
}

/// Typed local-cleanup failure with no credential or serialized storage data.
public enum SessionLogoutError: Error, Equatable, Sendable {
    case secureStorage(SecureCredentialSinkError)
    case credentialRemains
    case serviceUnavailable
}

/// Application boundary for forgetting the one locally stored device session.
public protocol SessionLogoutServiceProtocol: Sendable {
    func logoutLocally() async -> SessionLogoutResult
}

/// Deletes the active Keychain item and checks its absence without involving the server.
public struct SessionLogoutService: SessionLogoutServiceProtocol, Sendable {
    private let credentialStore: SecureCredentialSinkProtocol

    public init(credentialStore: SecureCredentialSinkProtocol) {
        self.credentialStore = credentialStore
    }

    public func logoutLocally() async -> SessionLogoutResult {
        do {
            try await credentialStore.delete()
        } catch let error as SecureCredentialSinkError where error == .itemNotFound {
            // An already-missing item is an idempotent local logout. Verify below before success.
        } catch let error as SecureCredentialSinkError {
            return .failed(.secureStorage(error))
        } catch {
            return .failed(.secureStorage(.deletionFailure))
        }

        do {
            guard try await credentialStore.isActiveCredentialAbsent() else {
                return .failed(.credentialRemains)
            }
        } catch let error as SecureCredentialSinkError {
            return .failed(.secureStorage(error))
        } catch {
            return .failed(.secureStorage(.readFailure))
        }

        return .credentialAbsent
    }
}
