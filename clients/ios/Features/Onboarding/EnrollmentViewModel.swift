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
    case succeeded
}

/// `@MainActor`-isolated ViewModel driving native SwiftUI device enrollment UI.
@Observable
@MainActor
public final class EnrollmentViewModel {
    public var rawTokenInput: String = "" {
        didSet {
            // Clear validation feedback when user modifies input
            if case .invalidToken = state {
                state = .idle
            }
        }
    }

    public private(set) var state: EnrollmentUIState = .idle
    public private(set) var isSubmitting: Bool = false

    private let sessionController: SessionController
    private let exchangeService: EnrollmentExchangeServiceProtocol
    private let credentialSink: SecureCredentialSinkProtocol
    private let rustBridge: RustBridgeProtocol?

    private var submitTask: Task<Void, Never>?

    /// Initializes `EnrollmentViewModel`.
    ///
    /// - Parameters:
    ///   - sessionController: Authoritative session controller holding the configured `serverEndpoint`.
    ///   - exchangeService: Device enrollment exchange HTTP service.
    ///   - credentialSink: Secure credential sink preflight and persistence boundary.
    ///   - rustBridge: Optional `RustBridgeProtocol` for local shared-core token validation.
    public init(
        sessionController: SessionController,
        exchangeService: EnrollmentExchangeServiceProtocol? = nil,
        credentialSink: SecureCredentialSinkProtocol? = nil,
        rustBridge: RustBridgeProtocol? = nil
    ) {
        self.sessionController = sessionController
        self.rustBridge = rustBridge
        self.credentialSink = credentialSink ?? StubSecureCredentialSink(isAvailable: true)

        if let service = exchangeService {
            self.exchangeService = service
        } else {
            let transport = URLSessionHTTPTransport()
            self.exchangeService = EnrollmentExchangeService(
                transport: transport,
                rustBridge: rustBridge
            )
        }
    }

    /// Submits the current enrollment token for exchange.
    ///
    /// Prevents duplicate concurrent exchange submissions and performs local token validation prior to
    /// network calls.
    public func submitEnrollment() {
        guard !isSubmitting else { return }

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

        submitTask = Task {
            // Local token validation using Rust bridge if available, falling back to local domain validation
            let isValidToken: Bool
            if let bridge = rustBridge {
                do {
                    isValidToken = try await bridge.validateEnrollmentToken(trimmedInput)
                } catch {
                    isValidToken = EnrollmentToken.isValid(trimmedInput)
                }
            } else {
                isValidToken = EnrollmentToken.isValid(trimmedInput)
            }

            guard isValidToken, let token = EnrollmentToken.parse(trimmedInput) else {
                self.state = .invalidToken(
                    "Enrollment token format is invalid. It must begin with 'sve1_' followed by 64 "
                        + "lowercase hex characters."
                )
                self.isSubmitting = false
                return
            }

            // Secure storage preflight check before consuming token over the network
            do {
                try await credentialSink.preflight()
            } catch {
                self.state = .secureStoreUnavailable(
                    "Secure credential storage is unavailable. No enrollment request was sent."
                )
                self.isSubmitting = false
                return
            }

            guard !Task.isCancelled else {
                self.isSubmitting = false
                return
            }

            // Perform single-shot exchange request
            let result = await exchangeService.exchange(endpoint: endpoint, token: token)

            guard !Task.isCancelled else {
                self.isSubmitting = false
                return
            }

            switch result {
            case .success(let record):
                do {
                    try await credentialSink.store(record)
                    self.state = .succeeded
                    self.isSubmitting = false
                    // NOTE: In Prompt024, successful exchange alone does NOT transition to .authenticated
                    // because durable Keychain persistence and session restoration are owned by Prompt025+.
                } catch {
                    self.state = .recoveryRequired(
                        "Credential storage failed following exchange. Recovery is required."
                    )
                    self.isSubmitting = false
                }

            case .rejected:
                self.state = .rejected("The enrollment grant was rejected by the server.")
                self.isSubmitting = false

            case .recoveryRequired:
                self.state = .recoveryRequired(
                    "The enrollment result is unknown. Use the trusted owner recovery workflow before "
                        + "trying again."
                )
                self.isSubmitting = false

            case .failed(let reason):
                switch reason {
                case .secureStorageUnavailable:
                    self.state = .secureStoreUnavailable("Secure storage unavailable.")
                case .invalidConfiguration:
                    self.state = .recoveryRequired("Enrollment configuration error.")
                case .cancelled:
                    self.state = .idle
                case .transport:
                    self.state = .recoveryRequired("Enrollment could not be completed safely.")
                }
                self.isSubmitting = false
            }
        }
    }

    /// Cancels active submission safely without advancing session state.
    public func cancel() {
        submitTask?.cancel()
        submitTask = nil
        isSubmitting = false
    }

    /// User-friendly message for UI display.
    public var errorMessage: String? {
        switch state {
        case .invalidToken(let msg): return msg
        case .rejected(let msg): return msg
        case .recoveryRequired(let msg): return msg
        case .secureStoreUnavailable(let msg): return msg
        case .idle, .submitting, .succeeded: return nil
        }
    }
}
