import Foundation
import Observation

/// UI state holder representing the current status of device enrollment.
public enum EnrollmentUIState: Sendable, Equatable {
    case idle
    case invalidToken(String)
    case submitting
    case rejected(String)
    case recoveryRequired(String)
    case secureStoreUnavailable(String)
    case securityServicesUnavailable(String)
    case succeeded
}

/// `@MainActor`-isolated ViewModel driving native SwiftUI device enrollment UI.
@Observable
@MainActor
public final class EnrollmentViewModel {
    public var rawTokenInput: String = "" {
        didSet {
            // Clear validation feedback when user modifies input
            switch state {
            case .invalidToken, .rejected:
                state = .idle
                recoveryRequestID = nil
            default: break
            }
        }
    }

    public private(set) var state: EnrollmentUIState = .idle
    public private(set) var isSubmitting: Bool = false

    private let sessionController: SessionController
    private let exchangeService: EnrollmentExchangeServiceProtocol?
    private let credentialSink: SecureCredentialSinkProtocol
    private let rustBridge: RustBridgeProtocol?

    public private(set) var recoveryRequestID: String?

    private var submitTask: Task<Void, Never>?

    /// Initializes `EnrollmentViewModel`.
    ///
    /// - Parameters:
    ///   - sessionController: Authoritative controller holding the configured `serverEndpoint`.
    ///   - exchangeService: Device enrollment exchange HTTP service.
    ///   - credentialSink: Secure credential sink preflight and persistence boundary.
    ///   - rustBridge: Authoritative validator. Production enrollment fails closed when absent.
    public init(
        sessionController: SessionController,
        exchangeService: EnrollmentExchangeServiceProtocol? = nil,
        credentialSink: SecureCredentialSinkProtocol? = nil,
        rustBridge: RustBridgeProtocol? = nil
    ) {
        self.sessionController = sessionController
        self.rustBridge = rustBridge
        self.credentialSink = credentialSink ?? StubSecureCredentialSink()

        if let service = exchangeService {
            self.exchangeService = service
        } else if let rustBridge {
            let transport = URLSessionHTTPTransport(maxResponseBodyBytes: 16 * 1024)
            self.exchangeService = EnrollmentExchangeService(
                transport: transport,
                rustBridge: rustBridge
            )
        } else {
            self.exchangeService = nil
        }
    }

    /// Submits the current enrollment token for exchange.
    ///
    /// Prevents duplicate concurrent submissions and validates the token before any network call.
    public func submitEnrollment() {
        guard !isSubmitting else { return }
        guard sessionController.state == .needsEnrollment else { return }

        let trimmedInput = rawTokenInput.trimmingCharacters(in: .whitespacesAndNewlines)

        guard !trimmedInput.isEmpty else {
            self.state = .invalidToken("Please enter an enrollment token.")
            return
        }

        guard let endpoint = sessionController.serverEndpoint else {
            self.state = .recoveryRequired("Server endpoint is not configured.")
            return
        }

        isSubmitting = true
        state = .submitting
        recoveryRequestID = nil
        let revision = sessionController.lifecycleRevision

        submitTask = Task {
            defer {
                self.isSubmitting = false
                self.submitTask = nil
            }
            guard self.isCurrentEnrollment(revision, endpoint: endpoint) else { return }
            // Rust shared-core validation is authoritative for production grant consumption.
            guard let bridge = rustBridge else {
                self.state = .securityServicesUnavailable(
                    "Authoritative enrollment validation is unavailable. "
                        + "No enrollment request was sent."
                )
                return
            }

            let isValidToken: Bool
            do {
                isValidToken = try await bridge.validateEnrollmentToken(trimmedInput)
            } catch {
                guard self.isCurrentEnrollment(revision, endpoint: endpoint) else { return }
                guard !Task.isCancelled else { self.state = .idle; return }
                self.state = .securityServicesUnavailable(
                    "Authoritative enrollment validation is unavailable. "
                        + "No enrollment request was sent."
                )
                return
            }

            guard self.isCurrentEnrollment(revision, endpoint: endpoint) else { return }
            guard !Task.isCancelled else { self.state = .idle; return }
            guard isValidToken, let token = EnrollmentToken.parse(trimmedInput) else {
                self.state = .invalidToken(
                    "Enrollment token format is invalid. It must begin with 'sve1_' followed by 64 "
                        + "lowercase hex characters."
                )
                return
            }

            guard let exchangeService else {
                self.state = .securityServicesUnavailable(
                    "Enrollment security services are unavailable. No enrollment request was sent."
                )
                return
            }

            // Secure storage preflight check before consuming token over the network.
            do {
                try await credentialSink.preflight()
            } catch {
                guard self.isCurrentEnrollment(revision, endpoint: endpoint) else { return }
                guard !Task.isCancelled else { self.state = .idle; return }
                self.state = .secureStoreUnavailable(
                    "Secure credential storage is unavailable. No enrollment request was sent."
                )
                return
            }

            guard self.isCurrentEnrollment(revision, endpoint: endpoint) else { return }
            guard !Task.isCancelled else {
                self.state = .idle
                return
            }

            // Perform single-shot exchange request
            let result = await exchangeService.exchange(endpoint: endpoint, token: token)

            guard self.isCurrentEnrollment(revision, endpoint: endpoint) else { return }
            guard !Task.isCancelled else {
                self.sessionController.requireRecovery(.enrollmentAmbiguous)
                self.state = .recoveryRequired(
                    "The enrollment result is unknown. Use the trusted owner recovery workflow "
                        + "before trying again."
                )
                return
            }

            switch result {
            case .success(let record):
                do {
                    let receipt = try await credentialSink.store(record, for: endpoint)
                    guard self.isCurrentEnrollment(revision, endpoint: endpoint) else { return }
                    guard !Task.isCancelled else {
                        self.sessionController.requireRecovery(.enrollmentAmbiguous)
                        return
                    }
                    self.sessionController.markAuthenticated(after: receipt)
                    guard self.sessionController.state == .authenticated else {
                        throw SecureCredentialSinkError.verificationFailure
                    }
                    self.state = .succeeded
                    } catch {
                    guard self.isCurrentEnrollment(revision, endpoint: endpoint) else { return }
                    self.sessionController.requireRecovery(.secureStore)
                    self.state = .recoveryRequired(
                        "Credential storage failed following exchange. Recovery is required."
                    )
                    }

            case .rejected(_, let requestID):
                self.recoveryRequestID = AuthenticationRecoveryPresenter.safeRequestID(requestID)
                self.rawTokenInput = ""
                self.state = .rejected("The enrollment grant was rejected by the server.")

            case .recoveryRequired:
                self.sessionController.requireRecovery(.enrollmentAmbiguous)
                self.state = .recoveryRequired(
                    "The enrollment result is unknown. Use the trusted owner recovery workflow "
                        + "before trying again."
                )

            case .failed(let reason):
                switch reason {
                case .secureStorageUnavailable:
                    self.state = .secureStoreUnavailable("Secure storage unavailable.")
                case .authoritativeValidationUnavailable:
                    self.state = .securityServicesUnavailable(
                        "Authoritative enrollment validation is unavailable. "
                            + "No enrollment request was sent."
                    )
                case .invalidConfiguration:
                    self.sessionController.requireRecovery(.configuration)
                    self.state = .recoveryRequired("Enrollment configuration error.")
                case .cancelled:
                    self.sessionController.requireRecovery(.enrollmentAmbiguous)
                    self.state = .recoveryRequired(
                        "The enrollment result is unknown. Use the trusted owner recovery workflow "
                            + "before trying again."
                    )
                case .transport:
                    self.sessionController.requireRecovery(.enrollmentAmbiguous)
                    self.state = .recoveryRequired("Enrollment could not be completed safely.")
                }
            }
        }
    }

    /// Cancels active submission safely without advancing session state.
    public func cancel() {
        submitTask?.cancel()
    }

    /// Awaits the owned single-shot operation without submitting a grant.
    func waitForCurrentSubmission() async {
        await submitTask?.value
    }

    private func isCurrentEnrollment(_ revision: UInt64, endpoint: ServerEndpoint) -> Bool {
        sessionController.lifecycleRevision == revision
            && sessionController.state == .needsEnrollment
            && sessionController.serverEndpoint == endpoint
    }

    var recoveryPresentation: AuthenticationRecoveryPresentation? {
        AuthenticationRecoveryPresenter.enrollment(
            state, inputIsEmpty: rawTokenInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
            requestID: recoveryRequestID)
    }

    /// User-friendly message for UI display; stored strings never supply presentation copy.
    public var errorMessage: String? {
        guard let presentation = recoveryPresentation else { return nil }
        return presentation.message + " " + presentation.nextStep
    }
}
