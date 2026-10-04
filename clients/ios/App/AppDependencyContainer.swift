import Foundation

/// Top-level application dependency composition root.
///
/// Responsible for constructing application dependencies and managing top-level singletons/controllers.
@MainActor
public final class AppDependencyContainer {
    /// Non-secret application configuration.
    public let configuration: AppConfiguration

    /// Authoritative application session controller.
    public let sessionController: SessionController

    /// Initializes application dependencies and constructs top-level services.
    ///
    /// - Parameter configuration: App configuration containing non-secret bootstrap settings. Defaults to `AppConfiguration.load()`.
    public init(configuration: AppConfiguration = AppConfiguration.load()) {
        self.configuration = configuration
        self.sessionController = SessionController(configuration: configuration)
    }
}
