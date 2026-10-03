import Foundation

/// Non-secret application configuration model.
///
/// Holds non-secret startup configuration parameters such as the active execution environment
/// and an optional default server base endpoint.
///
/// The safe production default guarantees that `serverEndpoint` is `nil` (unconfigured server)
/// unless explicitly injected or overridden by non-secret environment variables during local testing.
public struct AppConfiguration: Equatable, Hashable, Sendable {
    /// Environment identity (e.g., `.production`, `.development`, `.testing`).
    public let environment: AppEnvironment

    /// Optional bootstrap/default server endpoint.
    /// In production builds, this defaults to `nil`.
    public let serverEndpoint: ServerEndpoint?

    /// Key for optional non-secret server URL process environment variable override during local debugging/testing.
    public static let environmentServerURLKey = "SYNVEIL_SERVER_URL"

    /// Key for optional process environment variable environment override.
    public static let environmentNameKey = "SYNVEIL_APP_ENV"

    /// Initializes explicit immutable application configuration.
    ///
    /// - Parameters:
    ///   - environment: Execution environment identity. Defaults to `.production`.
    ///   - serverEndpoint: Optional default server base endpoint. Defaults to `nil`.
    public init(
        environment: AppEnvironment = .production,
        serverEndpoint: ServerEndpoint? = nil
    ) {
        self.environment = environment
        self.serverEndpoint = serverEndpoint
    }

    /// Loads application configuration using process environment variable overrides if present,
    /// falling back safely to unconfigured production defaults (`serverEndpoint = nil`).
    ///
    /// - Parameter processEnvironment: Dictionary representing process environment variables.
    /// - Returns: Loaded `AppConfiguration`.
    public static func load(
        processEnvironment: [String: String] = ProcessInfo.processInfo.environment
    ) -> AppConfiguration {
        let envString = processEnvironment[environmentNameKey]?.lowercased() ?? ""
        let environment = AppEnvironment(rawValue: envString) ?? .production

        var endpoint: ServerEndpoint? = nil
        if let rawEndpoint = processEnvironment[environmentServerURLKey],
            !rawEndpoint.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        {
            endpoint = try? ServerEndpoint(validating: rawEndpoint)
        }

        return AppConfiguration(
            environment: environment,
            serverEndpoint: endpoint
        )
    }
}
