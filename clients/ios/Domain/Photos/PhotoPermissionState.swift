import Foundation

public enum PhotoPermissionState: String, Codable, Sendable, Equatable {
    case notDetermined
    case authorizedFull
    case authorizedLimited
    case denied
    case restricted
    case unavailable
    case unknownFutureStatus

    public var isAuthorized: Bool {
        switch self {
        case .authorizedFull, .authorizedLimited:
            return true
        default:
            return false
        }
    }
}
