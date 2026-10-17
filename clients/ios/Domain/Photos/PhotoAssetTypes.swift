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

public enum PhotoResourceRendition: String, Codable, Sendable, Equatable {
    case original
    case edited
    case pairedMotion
    case auxiliary
}

public enum PhotoResourceAvailability: String, Codable, Sendable, Equatable {
    case unknown
    case localVerified
    case requiresICloud
    case unavailable
    case revoked
}

public enum PhotoSourceAvailability: String, Codable, Sendable, Equatable {
    case available
    case sourceUnavailable
    case accessRevoked
    case outsideLimitedSelection
    case identifierUnresolvable
}
