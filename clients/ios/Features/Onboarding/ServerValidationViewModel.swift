import Foundation
import Observation

/// UI state holder representing the current status of server validation probing.
public enum ServerValidationUIState: Sendable, Equatable {
    case idle
    case checking
    case ready
    case aliveButNotReady(requestId: String?, code: String?)
    case failed(SynveilTransportError)
}

/// `@MainActor`-isolated ViewModel driving server reachability validation.
@Observable
@MainActor
public final class ServerValidationViewModel {
    public private(set) var state: ServerValidationUIState = .idle
    public private(set) var isChecking: Bool = false

    private let sessionController: SessionController
    private let validationService: ServerValidationServiceProtocol
    private var validationTask: Task<Void, Never>?

    /// Initializes ViewModel with SessionController and ServerValidationService.
    ///
    /// - Parameters:
    ///   - sessionController: Authoritative session controller holding the configured `serverEndpoint`.
    ///   - validationService: Service for executing server reachability probe.
    public init(
        sessionController: SessionController,
        validationService: ServerValidationServiceProtocol? = nil
    ) {
        self.sessionController = sessionController
        if let service = validationService {
            self.validationService = service
        } else {
            let transport = URLSessionHTTPTransport()
            self.validationService = ServerValidationService(transport: transport)
        }
    }

    /// Initiates server reachability validation.
    ///
    /// Idempotent while validation is currently running.
    public func validateServer() {
        guard !isChecking else {
            return
        }

        guard let endpoint = sessionController.serverEndpoint else {
            self.state = .failed(.configurationError)
            return
        }

        isChecking = true
        state = .checking

        validationTask = Task {
            let result = await validationService.validateServer(endpoint: endpoint)

            guard !Task.isCancelled else {
                self.isChecking = false
                return
            }

            switch result {
            case .ready:
                self.state = .ready
                self.isChecking = false
                // Advance session state to .needsEnrollment upon confirmed readiness
                self.sessionController.requireEnrollment()

            case .aliveButNotReady(let requestId, let code):
                self.state = .aliveButNotReady(requestId: requestId, code: code)
                self.isChecking = false

            case .failure(let error):
                if case .cancelled = error {
                    self.isChecking = false
                    return
                }
                self.state = .failed(error)
                self.isChecking = false
            }
        }
    }

    /// Retries server validation by cancelling any ongoing task and starting a fresh probe sequence.
    public func retry() {
        cancelValidation()
        validateServer()
    }

    /// Cancels active validation task safely without advancing session state.
    public func cancelValidation() {
        validationTask?.cancel()
        validationTask = nil
        isChecking = false
    }

    /// Returns concise, user-friendly failure title and message for UI display.
    public var userFacingErrorMessage: (title: String, message: String) {
        switch state {
        case .idle, .checking, .ready:
            return ("", "")
        case .aliveButNotReady(_, let code):
            let details = code != nil ? " (Code: \(code!))" : ""
            return (
                "Server Not Ready",
                "The server was reached but is currently undergoing maintenance or "
                    + "initialization\(details). Please try again in a few moments."
            )
        case .failed(let error):
            switch error {
            case .offline:
                return (
                    "Connection Error",
                    "Unable to connect to the server. Please check your network connection "
                        + "and server address."
                )
            case .dnsFailure:
                return (
                    "Server Not Found",
                    "Could not resolve the server address. Please verify the host name."
                )
            case .timeout:
                return (
                    "Request Timed Out",
                    "The server did not respond in time. Please try again."
                )
            case .tlsError:
                return (
                    "Security Failure",
                    "A secure TLS connection could not be established with the server."
                )
            case .redirectRejected:
                return (
                    "Redirect Rejected",
                    "The server attempted an unexpected redirect, which is prohibited "
                        + "for security."
                )
            case .unexpectedContentType:
                return (
                    "Incompatible Service",
                    "The server responded with an invalid payload format. Please ensure "
                        + "the URL points to a Synveil server."
                )
            case .bodyLimitExceeded:
                return (
                    "Response Too Large",
                    "The server health response exceeded the expected maximum size limit."
                )
            case .malformedResponse, .protocolError:
                return (
                    "Incompatible Server",
                    "The endpoint responded but does not appear to run a compatible "
                        + "Synveil service."
                )
            case .httpError(let statusCode, let code, _):
                let codeDetail = code != nil ? " (\(code!))" : ""
                return (
                    "Server Error",
                    "Server returned HTTP error status \(statusCode)\(codeDetail)."
                )
            case .configurationError:
                return (
                    "Configuration Error",
                    "Invalid server configuration or transport policy."
                )
            case .cancelled:
                return ("Cancelled", "Validation was cancelled.")
            }
        }
    }

}
