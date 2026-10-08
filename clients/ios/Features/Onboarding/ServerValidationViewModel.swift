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
    private var operationRevision: UInt64 = 0
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
        guard !isChecking, sessionController.state == .readyForServerValidation else {
            return
        }

        guard let endpoint = sessionController.serverEndpoint else {
            self.state = .failed(.configurationError)
            return
        }

        isChecking = true
        state = .checking
        operationRevision &+= 1
        let operation = operationRevision
        let revision = sessionController.lifecycleRevision

        validationTask = Task {
            let result = await validationService.validateServer(endpoint: endpoint)

            guard operation == self.operationRevision else { return }
            guard sessionController.lifecycleRevision == revision,
                sessionController.state == .readyForServerValidation,
                sessionController.serverEndpoint == endpoint
            else {
                self.isChecking = false
                return
            }
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

    /// Retries server validation once the preceding probe sequence has completed.
    public func retry() {
        guard !isChecking else { return }
        validateServer()
    }

    /// Cancels active validation task safely without advancing session state.
    public func cancelValidation() {
        operationRevision &+= 1
        validationTask?.cancel()
        validationTask = nil
        isChecking = false
        if case .checking = state { state = .idle }
    }

    /// Read-only completion handle for deterministic lifecycle observers.
    var currentValidationTask: Task<Void, Never>? { validationTask }

    /// Awaits the owned operation without creating or retrying a request.
    func waitForCurrentValidation() async {
        await validationTask?.value
    }

    var recoveryPresentation: AuthenticationRecoveryPresentation? {
        AuthenticationRecoveryPresenter.serverValidation(state)
    }

    /// Compatibility accessor, backed by the centralized safe presentation.
    public var userFacingErrorMessage: (title: String, message: String) {
        guard let presentation = recoveryPresentation else { return ("", "") }
        return (presentation.title, presentation.message + " " + presentation.nextStep)
    }
}
