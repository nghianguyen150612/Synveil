import Foundation

public enum PhotoBackupScope: String, Codable, Sendable, Equatable {
    case allAuthorized
    case limitedSelection
}

public struct PhotoBackupConfiguration: Codable, Sendable, Equatable {
    public var isOptedIn: Bool
    public var scope: PhotoBackupScope
    public var includeVideos: Bool
    public var pauseDiscovery: Bool

    public init(
        isOptedIn: Bool = false,
        scope: PhotoBackupScope = .allAuthorized,
        includeVideos: Bool = true,
        pauseDiscovery: Bool = false
    ) {
        self.isOptedIn = isOptedIn
        self.scope = scope
        self.includeVideos = includeVideos
        self.pauseDiscovery = pauseDiscovery
    }
}

public enum PhotoDiscoveryScanResult: Sendable, Equatable {
    case completed(totalObserved: Int, generation: Int64)
    case partial(observedSoFar: Int, totalInScope: Int, generation: Int64)
    case interrupted(reason: String)
    case permissionDenied(PhotoPermissionState)
    case disabled
}
