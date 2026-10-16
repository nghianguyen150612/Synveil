import Foundation

public enum PhotoAssetKind: String, Codable, Sendable, Equatable {
    case stillPhoto
    case video
    case livePhoto
    case screenshot
    case unknown
}

public enum PhotoResourceRole: String, Codable, Sendable, Equatable {
    case primaryImage
    case primaryVideo
    case motionVideo
    case auxiliary
}

public enum PhotoSourceAvailability: String, Codable, Sendable, Equatable {
    case available
    case sourceUnavailable
    case accessRevoked
    case outsideLimitedSelection
    case identifierUnresolvable
}
