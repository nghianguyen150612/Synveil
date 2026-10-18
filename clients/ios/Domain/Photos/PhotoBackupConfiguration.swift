import Foundation

public enum PhotoBackupScope: String, Codable, Sendable, Equatable {
    case allAuthorized
    case limitedSelection
}

public enum PhotoBackupRenditionPolicy: String, Codable, Sendable, Equatable {
    case originalOnly
    case originalAndEdited
}

public enum PhotoBackupNetworkRestriction: String, Codable, Sendable, Equatable {
    case wifiOnly
    case wifiAndCellular
}

public struct PhotoBackupConfiguration: Codable, Sendable, Equatable {
    /// Compatibility name retained for P056/P057 callers. This controls discovery only.
    public var isOptedIn: Bool
    public var discoveryEnabled: Bool {
        get { isOptedIn }
        set { isOptedIn = newValue }
    }
    public var scope: PhotoBackupScope
    public var includeStillImages: Bool
    public var includeVideos: Bool
    public var includeLivePhotos: Bool
    public var renditionPolicy: PhotoBackupRenditionPolicy
    public var targetLibraryIdentifier: String?
    public var targetParentIdentifier: String?
    public var networkRestriction: PhotoBackupNetworkRestriction
    public var allowCellularTransfer: Bool
    public var respectLowDataMode: Bool
    public var maximumResourceSizeBytes: UInt64
    public var maximumConcurrentOperations: Int
    public var automaticForegroundBackupEnabled: Bool
    public var automaticBackupConsentConfirmed: Bool
    public var pauseAutomaticBackup: Bool
    public var policyRevision: Int64
    public var pauseDiscovery: Bool

    public var canAutomaticallyBackUpInForeground: Bool {
        discoveryEnabled && automaticForegroundBackupEnabled && automaticBackupConsentConfirmed
            && !pauseDiscovery && !pauseAutomaticBackup
            && targetLibraryIdentifier?.isEmpty == false
            && targetParentIdentifier?.isEmpty == false
    }

    public init(
        isOptedIn: Bool = false,
        scope: PhotoBackupScope = .allAuthorized,
        includeStillImages: Bool = true,
        includeVideos: Bool = true,
        includeLivePhotos: Bool = true,
        renditionPolicy: PhotoBackupRenditionPolicy = .originalOnly,
        targetLibraryIdentifier: String? = nil,
        targetParentIdentifier: String? = nil,
        networkRestriction: PhotoBackupNetworkRestriction = .wifiOnly,
        allowCellularTransfer: Bool = false,
        respectLowDataMode: Bool = true,
        maximumResourceSizeBytes: UInt64 = 536_870_912,
        maximumConcurrentOperations: Int = 1,
        automaticForegroundBackupEnabled: Bool = false,
        automaticBackupConsentConfirmed: Bool = false,
        pauseAutomaticBackup: Bool = false,
        policyRevision: Int64 = 1,
        pauseDiscovery: Bool = false,
        discoveryEnabled: Bool? = nil
    ) {
        self.isOptedIn = discoveryEnabled ?? isOptedIn
        self.scope = scope
        self.includeStillImages = includeStillImages
        self.includeVideos = includeVideos
        self.includeLivePhotos = includeLivePhotos
        self.renditionPolicy = renditionPolicy
        self.targetLibraryIdentifier = targetLibraryIdentifier
        self.targetParentIdentifier = targetParentIdentifier
        self.networkRestriction = networkRestriction
        self.allowCellularTransfer = allowCellularTransfer
        self.respectLowDataMode = respectLowDataMode
        self.maximumResourceSizeBytes = min(maximumResourceSizeBytes, UInt64(Int64.max))
        self.maximumConcurrentOperations = 1
        self.automaticForegroundBackupEnabled = automaticForegroundBackupEnabled
        self.automaticBackupConsentConfirmed = automaticBackupConsentConfirmed
        self.pauseAutomaticBackup = pauseAutomaticBackup
        self.policyRevision = max(policyRevision, 1)
        self.pauseDiscovery = pauseDiscovery
    }

    public func replacingPolicyRevision(_ revision: Int64) -> PhotoBackupConfiguration {
        var value = self
        value.policyRevision = max(revision, 1)
        return value
    }
}

public enum PhotoDiscoveryScanResult: Sendable, Equatable {
    case completed(totalObserved: Int, generation: Int64)
    case partial(observedSoFar: Int, totalInScope: Int, generation: Int64)
    case interrupted(reason: String)
    case permissionDenied(PhotoPermissionState)
    case disabled
}
