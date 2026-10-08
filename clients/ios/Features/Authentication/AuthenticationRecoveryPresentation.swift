import Foundation

/// Presentation semantics only. Authorization and lifecycle decisions stay in application services.
enum AuthenticationRecoveryCategory: String, CaseIterable, Sendable {
    case configuration, authentication, deviceRevoked, secureStoreUnavailable, secureStoreFailure
    case corruptRecord, unsupportedRecord, credential, scopeMismatch, tls, protocolFailure
    case enrollmentAmbiguous, offline, dns, timeout, serverUnavailable, verificationPending
    case emptyToken, malformedToken, enrollmentRejected, enrollmentStorage, securityServices
    case logoutCleanup
}

enum AuthenticationRecoverySeverity: Equatable, Sendable {
    case temporary, attention, security

    var symbol: String {
        switch self {
        case .temporary: return "network"
        case .attention: return "exclamationmark.triangle"
        case .security: return "lock.trianglebadge.exclamationmark"
        }
    }
}

/// Only implemented, bounded retry operations are represented here; enrollment replay is absent.
enum AuthenticationRecoveryAction: Equatable, Sendable {
    case retryVerification, retryCleanup, retryServerValidation

    var label: String {
        switch self {
        case .retryVerification: return "Retry Verification"
        case .retryCleanup: return "Retry Cleanup"
        case .retryServerValidation: return "Retry Connection"
        }
    }

    var accessibilityIdentifier: String {
        switch self {
        case .retryVerification: return "synveil.root.restoration-retry"
        case .retryCleanup: return "synveil.logout.retry"
        case .retryServerValidation: return "synveil.server-validation.retry-button"
        }
    }

    var hint: String {
        switch self {
        case .retryVerification: return "Checks the saved session with its original server."
        case .retryCleanup: return "Retries verified local credential deletion. No network request is made."
        case .retryServerValidation: return "Checks server reachability and readiness before enrollment."
        }
    }
}

struct AuthenticationRecoveryPresentation: Equatable, Sendable {
    let category: AuthenticationRecoveryCategory
    let title: String
    let message: String
    let nextStep: String
    let sessionProtection: String
    var primaryAction: AuthenticationRecoveryAction? = nil
    var severity: AuthenticationRecoverySeverity = .attention
    var requestID: String? = nil

    var accessibilityDescription: String {
        "\(title). \(message) \(sessionProtection) \(nextStep)"
    }

    var accessibilityIdentifier: String { "synveil.recovery.\(category.rawValue)" }
}

/// Pure mapping from trusted typed classifications, never from server error text or raw Swift errors.
enum AuthenticationRecoveryPresenter {
    private static let retained = "The saved credential has not been deleted. Session access is blocked."
    private static let owner = "Contact your trusted server owner to restore access through authorized enrollment."

    static func recovery(
        _ reason: AppRecoveryReason,
        storageFailure: SessionSecureStorageFailure? = nil
    ) -> AuthenticationRecoveryPresentation {
        switch reason {
        case .authentication:
            return .init(
                category: .authentication, title: "Device authorization rejected",
                message: "The server cannot accept this device's saved authorization.",
                nextStep: owner + " You may need a new one-time enrollment grant.",
                sessionProtection: retained, severity: .security)
        case .deviceRevoked:
            return .init(
                category: .deviceRevoked, title: "Device access revoked",
                message: "The server no longer authorizes this device. Retrying the same credential will not restore access.",
                nextStep: owner, sessionProtection: retained, severity: .security)
        case .secureStore:
            return secureStorage(storageFailure)
        case .credential:
            return .init(
                category: .credential, title: "Saved authorization needs recovery",
                message: "The saved device credential could not be validated.",
                nextStep: owner, sessionProtection: retained, severity: .security)
        case .scopeMismatch:
            return .init(
                category: .scopeMismatch, title: "Session belongs to another server",
                message: "The saved device session does not match the configured server.",
                nextStep: "Check the original server configuration with your trusted owner. Automatic migration is unavailable.",
                sessionProtection: "The credential has not been sent to the different server or deleted.",
                severity: .security)
        case .tls:
            return .init(
                category: .tls, title: "Secure connection could not be verified",
                message: "Synveil cannot establish a trusted secure connection to the server.",
                nextStep: "Ask the trusted server owner to check the HTTPS certificate and configuration.",
                sessionProtection: retained, severity: .security)
        case .protocolFailure:
            return .init(
                category: .protocolFailure, title: "Server response could not be verified",
                message: "The server response does not satisfy the Synveil protocol.",
                nextStep: "Ask the server owner to check Synveil compatibility and configuration.",
                sessionProtection: retained)
        case .enrollmentAmbiguous:
            return .init(
                category: .enrollmentAmbiguous, title: "Enrollment result unconfirmed",
                message: "The result could not be confirmed. The one-time grant may already have been consumed.",
                nextStep: "Do not resend this grant. Contact the trusted owner for recovery; they may need to revoke affected credentials and issue a new grant.",
                sessionProtection: "Access remains blocked. Enrollment has not been confirmed.",
                severity: .security)
        case .configuration:
            return .init(
                category: .configuration, title: "Server setup needs attention",
                message: "The app cannot continue with the current server configuration.",
                nextStep: "Review the trusted server setup with its owner.", sessionProtection: retained)
        case .transport:
            return .init(
                category: .offline, title: "Cannot reach the server",
                message: "A connection to the server could not be completed.",
                nextStep: "Check your connection and the server's availability.",
                sessionProtection: retained, severity: .temporary)
        }
    }

    private static func secureStorage(
        _ failure: SessionSecureStorageFailure?
    ) -> AuthenticationRecoveryPresentation {
        switch failure {
        case .unavailable:
            return .init(
                category: .secureStoreUnavailable, title: "Secure storage is unavailable",
                message: "The device cannot currently access its protected session storage.",
                nextStep: "Unlock the device, then reopen Synveil to check secure storage again. If this persists, seek device support.",
                sessionProtection: retained)
        case .corruptRecord:
            return .init(
                category: .corruptRecord, title: "Saved session could not be read",
                message: "The stored session is damaged or could not be verified.",
                nextStep: owner + " The stored session will not be silently replaced.",
                sessionProtection: retained, severity: .security)
        case .unsupportedRecord:
            return .init(
                category: .unsupportedRecord, title: "Saved session format is unsupported",
                message: "This app cannot safely read the stored session format.",
                nextStep: "Check that Synveil is up to date, then contact your trusted owner for recovery.",
                sessionProtection: retained, severity: .security)
        case .readFailure, .failure, nil:
            return .init(
                category: .secureStoreFailure, title: "Secure storage needs attention",
                message: "A device security or storage operation could not be verified.",
                nextStep: "Unlock the device and reopen Synveil. If the problem continues, contact device support and your trusted owner.",
                sessionProtection: "Access remains blocked. Any stored credential has not been silently replaced or deleted.",
                severity: .security)
        }
    }

    static func restoration(
        _ failure: SessionRestorationVerificationFailure?
    ) -> AuthenticationRecoveryPresentation {
        let category: AuthenticationRecoveryCategory
        let title: String
        let message: String
        let nextStep: String
        switch failure {
        case .networkFailure:
            category = .offline
            title = "Cannot reach the server"
            message = "The saved session is waiting for a connection check."
            nextStep = "Check your internet connection, then retry verification."
        case .dnsFailure:
            category = .dns
            title = "Server address could not be found"
            message = "The network could not locate the configured server."
            nextStep = "Check your network and ask the owner to verify the server address, then retry verification."
        case .timeout:
            category = .timeout
            title = "Server verification timed out"
            message = "The server did not respond in time to verify the saved session."
            nextStep = "Check your connection and retry verification when the server is reachable."
        case .serverUnavailable:
            category = .serverUnavailable
            title = "Server temporarily unavailable"
            message = "The server is currently unable to verify the saved session."
            nextStep = "Wait a moment, then retry verification. Contact the owner if this continues."
        case .authenticationRejected: return recovery(.authentication)
        case .deviceRevoked: return recovery(.deviceRevoked)
        case .tlsFailure: return recovery(.tls)
        case .unexpectedContentType, .unexpectedProtocolResponse, .redirectRejected:
            return recovery(.protocolFailure)
        case .cancelled, nil:
            category = .verificationPending
            title = "Session verification pending"
            message = "Verification has not completed. This does not mean your authorization was rejected."
            nextStep = "Retry verification when you are ready."
        }
        return .init(
            category: category, title: title, message: message, nextStep: nextStep,
            sessionProtection: "The saved credential is retained. Access waits for server verification.",
            primaryAction: .retryVerification, severity: .temporary)
    }

    static var logoutCleanup: AuthenticationRecoveryPresentation {
        .init(
            category: .logoutCleanup, title: "Secure cleanup needs attention",
            message: "Synveil could not verify that the saved credential was deleted. Logout has not completed.",
            nextStep: "Retry local cleanup. This does not revoke authorization on the server.",
            sessionProtection: "Authenticated work remains blocked until deletion is verified.",
            primaryAction: .retryCleanup, severity: .security)
    }

    static func enrollment(
        _ state: EnrollmentUIState, inputIsEmpty: Bool, requestID: String? = nil
    ) -> AuthenticationRecoveryPresentation? {
        var presentation: AuthenticationRecoveryPresentation
        switch state {
        case .invalidToken:
            presentation = .init(
                category: inputIsEmpty ? .emptyToken : .malformedToken,
                title: inputIsEmpty ? "Enter an enrollment token" : "Check the token format",
                message: inputIsEmpty ? "An enrollment token is required." : "This input is not a valid enrollment token.",
                nextStep: "Enter sve1_ followed by exactly 64 lowercase hexadecimal characters (0–9, a–f). You can correct the input and submit again.",
                sessionProtection: "No enrollment request was sent.")
        case .rejected:
            presentation = .init(
                category: .enrollmentRejected, title: "Enrollment grant rejected",
                message: "The server did not accept this one-time grant.",
                nextStep: "Request a new grant through your trusted server owner's authorized workflow.",
                sessionProtection: "Device access has not been authorized.")
        case .recoveryRequired: presentation = recovery(.enrollmentAmbiguous)
        case .secureStoreUnavailable:
            presentation = .init(
                category: .enrollmentStorage, title: "Secure storage is unavailable",
                message: "Synveil cannot currently save a protected device session.",
                nextStep: "Unlock the device and try enrollment again once secure storage is available.",
                sessionProtection: "No enrollment request was sent. The grant has not been consumed by this attempt.")
        case .securityServicesUnavailable:
            presentation = .init(
                category: .securityServices, title: "Device security check unavailable",
                message: "Synveil could not validate enrollment safely on this device.",
                nextStep: "Reopen Synveil and check the app installation if this persists.",
                sessionProtection: "No enrollment request was sent.", severity: .security)
        case .idle, .submitting, .succeeded: return nil
        }
        presentation.requestID = safeRequestID(requestID)
        return presentation
    }

    /// Additional UI allowlist rejects secret-shaped IDs even if a transport accepted their grammar.
    static func safeRequestID(_ value: String?) -> String? {
        guard let value, (8...128).contains(value.utf8.count),
            value.range(of: "^[A-Za-z0-9._~-]+$", options: .regularExpression) != nil,
            !value.lowercased().contains("sve1_"), !value.lowercased().contains("svd1_"),
            value.range(of: "[0-9A-Fa-f]{64}", options: .regularExpression) == nil,
            !value.lowercased().contains("bearer"), !value.lowercased().contains("authorization")
        else { return nil }
        return value
    }

    static func serverValidation(
        _ state: ServerValidationUIState
    ) -> AuthenticationRecoveryPresentation? {
        var presentation: AuthenticationRecoveryPresentation
        var requestID: String?
        switch state {
        case .aliveButNotReady(let id, _):
            requestID = id
            presentation = restoration(.serverUnavailable)
        case .failed(let error):
            switch error {
            case .offline: presentation = restoration(.networkFailure)
            case .dnsFailure: presentation = restoration(.dnsFailure)
            case .timeout: presentation = restoration(.timeout)
            case .tlsError: presentation = recovery(.tls)
            case .configurationError: presentation = recovery(.configuration)
            case .httpError(let status, _, let id):
                requestID = id
                presentation = status == 503 ? restoration(.serverUnavailable) : recovery(.protocolFailure)
            case .redirectRejected, .unexpectedContentType, .bodyLimitExceeded,
                .malformedResponse, .protocolError:
                presentation = recovery(.protocolFailure)
            case .cancelled: return nil
            }
        case .idle, .checking, .ready: return nil
        }
        // Pre-auth checks have not established a session. Never claim there is a saved credential.
        return .init(
            category: presentation.category, title: presentation.title, message: presentation.severity == .temporary
                ? "Synveil could not complete the server readiness check."
                : presentation.message,
            nextStep: presentation.severity == .temporary
                ? "Check the network and server availability, then retry the connection check."
                : presentation.nextStep,
            sessionProtection: "Device enrollment remains blocked until the server is verified ready.",
            primaryAction: .retryServerValidation, severity: presentation.severity,
            requestID: safeRequestID(requestID))
    }
}
