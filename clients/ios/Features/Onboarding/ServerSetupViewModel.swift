import Foundation
import Observation

/// MainActor-isolated ViewModel managing server address entry and local validation.
///
/// Handles user input, trimming, local endpoint validation using `ServerEndpoint`,
/// mapping domain validation errors to actionable user messages, and updating
/// the application session state.
@Observable
@MainActor
public final class ServerSetupViewModel {
    /// Raw text input entered by the user.
    public var serverAddressInput: String {
        didSet {
            if validationError != nil {
                validationError = nil
            }
        }
    }

    /// User-facing validation error message, if input validation failed.
    public private(set) var validationError: String?

    /// Indicates whether a validation action is in progress.
    public private(set) var isValidating: Bool = false

    /// Authoritative application session controller reference.
    private let sessionController: SessionController

    /// Initializes `ServerSetupViewModel`.
    ///
    /// - Parameters:
    ///   - sessionController: The application session controller.
    ///   - initialInput: Optional initial server address string. Defaults to empty.
    public init(sessionController: SessionController, initialInput: String = "") {
        self.sessionController = sessionController
        self.serverAddressInput = initialInput
    }

    /// Validates the entered server address and configures the session endpoint if valid.
    ///
    /// - Returns: `true` if validation succeeded and state updated, `false` otherwise.
    @discardableResult
    public func submit() -> Bool {
        isValidating = true
        defer { isValidating = false }

        let trimmedInput = serverAddressInput.trimmingCharacters(in: .whitespacesAndNewlines)

        do {
            let endpoint = try ServerEndpoint(validating: trimmedInput)
            validationError = nil
            sessionController.configureServerEndpoint(endpoint)
            return true
        } catch let error as EndpointValidationError {
            validationError = userFriendlyMessage(for: error)
            return false
        } catch {
            validationError = "Invalid server address structure. Please enter a complete URL."
            return false
        }
    }

    /// Clears any currently displayed validation error.
    public func clearValidationError() {
        validationError = nil
    }

    // MARK: - Error Mapping

    private func userFriendlyMessage(for error: EndpointValidationError) -> String {
        switch error {
        case .empty:
            return "Please enter a server address."
        case .invalidURL:
            return "Invalid server address structure. Please enter a complete URL (e.g. https://synveil.example.com)."
        case .unsupportedScheme(let scheme):
            if scheme.isEmpty {
                return "Server address requires a URL scheme (e.g. https://synveil.example.com)."
            } else {
                return "Unsupported URL scheme '\(scheme)'. Synveil requires 'https' or 'http'."
            }
        case .missingHost:
            return "Server address is missing a valid hostname or IP address."
        case .userinfoNotAllowed:
            return "Server address must not contain username or password credentials."
        case .queryNotAllowed:
            return "Server address must not contain query parameters."
        case .fragmentNotAllowed:
            return "Server address must not contain URL fragments."
        case .invalidPath(let reason):
            return "Invalid server address path: \(reason)"
        }
    }
}
