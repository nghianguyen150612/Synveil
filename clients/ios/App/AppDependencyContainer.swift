import Foundation
import Observation

/// Top-level application dependency composition root.
///
/// Responsible for constructing application dependencies and managing top-level
/// singletons/controllers.
@Observable
@MainActor
public final class AppDependencyContainer {
    /// Non-secret application configuration.
    public let configuration: AppConfiguration

    /// Authoritative application session controller.
    public let sessionController: SessionController

    /// Authoritative Rust validator used by production enrollment and secure credential loading.
    public private(set) var rustBridge: (any RustBridgeProtocol)?

    /// Real Keychain store, available only after Rust validation initializes successfully.
    public private(set) var credentialSink: SecureCredentialSinkProtocol?

    private var enrollmentSecurityPrepared = false

    /// Initializes application dependencies and constructs top-level services.
    ///
    /// - Parameter configuration: App configuration containing non-secret bootstrap settings.
    ///   Defaults to `AppConfiguration.load()`.
    public init(configuration: AppConfiguration = AppConfiguration.load()) {
        self.configuration = configuration
        self.sessionController = SessionController(configuration: configuration)
    }

    /// Initializes production enrollment security dependencies. Failure leaves enrollment closed.
    public func prepareEnrollmentSecurity() async {
        guard !enrollmentSecurityPrepared else { return }
        enrollmentSecurityPrepared = true

        do {
            let bridge = try await RustBridgeAsyncAdapter()
            rustBridge = bridge
            credentialSink = KeychainCredentialStore(rustBridge: bridge)
        } catch {
            rustBridge = nil
            credentialSink = nil
        }
    }
}
