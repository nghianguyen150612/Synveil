import Foundation

/// Represents the execution runtime environment for the Synveil application.
public enum AppEnvironment: String, Equatable, Hashable, Sendable, CustomStringConvertible {
    case production
    case development
    case testing

    public var description: String {
        rawValue
    }
}
