import Foundation

public struct PhotoAssetObservation: Codable, Sendable, Equatable, Identifiable {
    public var id: String { localIdentifier }

    public let localIdentifier: String
    public let assetKind: PhotoAssetKind
    public let creationDate: Date?
    public let modificationDate: Date?
    public let pixelWidth: Int
    public let pixelHeight: Int
    public let isFavorite: Bool
    public let isHidden: Bool
    public let discoveryGeneration: Int64
    public let observedAt: Date
    public let permissionScope: PhotoPermissionState
    public let sourceAvailability: PhotoSourceAvailability
    public let isExcluded: Bool

    public init(
        localIdentifier: String,
        assetKind: PhotoAssetKind,
        creationDate: Date?,
        modificationDate: Date?,
        pixelWidth: Int,
        pixelHeight: Int,
        isFavorite: Bool,
        isHidden: Bool,
        discoveryGeneration: Int64,
        observedAt: Date,
        permissionScope: PhotoPermissionState,
        sourceAvailability: PhotoSourceAvailability,
        isExcluded: Bool = false
    ) {
        self.localIdentifier = localIdentifier
        self.assetKind = assetKind
        self.creationDate = creationDate
        self.modificationDate = modificationDate
        self.pixelWidth = pixelWidth
        self.pixelHeight = pixelHeight
        self.isFavorite = isFavorite
        self.isHidden = isHidden
        self.discoveryGeneration = discoveryGeneration
        self.observedAt = observedAt
        self.permissionScope = permissionScope
        self.sourceAvailability = sourceAvailability
        self.isExcluded = isExcluded
    }
}

public struct PhotoAssetResourceDescriptor: Codable, Sendable, Equatable, Identifiable {
    public var id: String { "\(localIdentifier)_\(resourceIndex)_\(resourceType)" }

    public let localIdentifier: String
    public let role: PhotoResourceRole
    public let resourceIndex: Int
    public let resourceType: String
    public let rendition: PhotoResourceRendition
    public let originalFilename: String?
    public let uniformTypeIdentifier: String?
    public let availability: PhotoResourceAvailability
    public let fileSizeEstimateBytes: Int64?

    public var isLocallyAvailable: Bool { availability == .localVerified }

    public init(
        localIdentifier: String,
        role: PhotoResourceRole,
        resourceIndex: Int = 0,
        resourceType: String = "unknown",
        rendition: PhotoResourceRendition = .original,
        originalFilename: String?,
        uniformTypeIdentifier: String?,
        availability: PhotoResourceAvailability = .unknown,
        fileSizeEstimateBytes: Int64?
    ) {
        self.localIdentifier = localIdentifier
        self.role = role
        self.resourceIndex = resourceIndex
        self.resourceType = resourceType
        self.rendition = rendition
        self.originalFilename = originalFilename
        self.uniformTypeIdentifier = uniformTypeIdentifier
        self.availability = availability
        self.fileSizeEstimateBytes = fileSizeEstimateBytes
    }

}

public struct PhotoTransferCandidateDescriptor: Codable, Sendable, Equatable, Identifiable {
    public var id: String { candidateId.uuidString }

    public let candidateId: UUID
    public let profileId: UUID
    public let ownerId: String
    public let deviceId: String
    public let localIdentifier: String
    public let assetKind: PhotoAssetKind
    public let resourceRole: PhotoResourceRole
    public let proposedFileName: String
    public let estimatedSizeBytes: Int64?
    public let observationGeneration: Int64
    public let createdAt: Date

    public init(
        candidateId: UUID = UUID(),
        profileId: UUID,
        ownerId: String,
        deviceId: String,
        localIdentifier: String,
        assetKind: PhotoAssetKind,
        resourceRole: PhotoResourceRole,
        proposedFileName: String,
        estimatedSizeBytes: Int64?,
        observationGeneration: Int64,
        createdAt: Date = Date()
    ) {
        self.candidateId = candidateId
        self.profileId = profileId
        self.ownerId = ownerId
        self.deviceId = deviceId
        self.localIdentifier = localIdentifier
        self.assetKind = assetKind
        self.resourceRole = resourceRole
        self.proposedFileName = proposedFileName
        self.estimatedSizeBytes = estimatedSizeBytes
        self.observationGeneration = observationGeneration
        self.createdAt = createdAt
    }
}
