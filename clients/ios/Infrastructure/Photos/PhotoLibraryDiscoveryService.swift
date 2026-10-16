import Foundation
import Photos

public final class PhotoLibraryDiscoveryService: PhotoLibraryDiscoveryProtocol,
    @unchecked Sendable
{
    private let permissionAdapter: PhotoLibraryPermissionChecking

    public init(
        permissionAdapter: PhotoLibraryPermissionChecking = PhotoLibraryPermissionAdapter()
    ) {
        self.permissionAdapter = permissionAdapter
    }

    public func discoverAssets(
        configuration: PhotoBackupConfiguration,
        generation: Int64,
        batchSize: Int = 500
    ) async throws -> [PhotoAssetObservation] {
        guard configuration.isOptedIn && !configuration.pauseDiscovery else {
            return []
        }

        let permission = await permissionAdapter.currentPermissionState()
        guard permission.isAuthorized else {
            return []
        }

        let fetchOptions = PHFetchOptions()
        fetchOptions.sortDescriptors = [NSSortDescriptor(key: "creationDate", ascending: true)]

        var predicates: [NSPredicate] = []
        if !configuration.includeVideos {
            predicates.append(
                NSPredicate(format: "mediaType == %d", PHAssetMediaType.image.rawValue)
            )
        }
        if !predicates.isEmpty {
            fetchOptions.predicate = NSCompoundPredicate(andPredicateWithSubpredicates: predicates)
        }

        let fetchResult = PHAsset.fetchAssets(with: fetchOptions)
        var observations: [PhotoAssetObservation] = []
        let totalCount = fetchResult.count
        let limit = batchSize > 0 ? min(totalCount, batchSize) : totalCount

        let now = Date()

        for idx in 0..<limit {
            let asset = fetchResult.object(at: idx)
            let kind = classifyAsset(asset)
            let observation = PhotoAssetObservation(
                localIdentifier: asset.localIdentifier,
                assetKind: kind,
                creationDate: asset.creationDate,
                modificationDate: asset.modificationDate,
                pixelWidth: asset.pixelWidth,
                pixelHeight: asset.pixelHeight,
                isFavorite: asset.isFavorite,
                isHidden: asset.isHidden,
                discoveryGeneration: generation,
                observedAt: now,
                permissionScope: permission,
                sourceAvailability: .available,
                isExcluded: false
            )
            observations.append(observation)
        }

        return observations
    }

    public func discoverResourceDescriptors(
        for localIdentifier: String
    ) async throws -> [PhotoAssetResourceDescriptor] {
        let fetchResult = PHAsset.fetchAssets(
            withLocalIdentifiers: [localIdentifier],
            options: nil
        )
        guard let asset = fetchResult.firstObject else {
            return []
        }

        let resources = PHAssetResource.assetResources(for: asset)
        var descriptors: [PhotoAssetResourceDescriptor] = []

        for res in resources {
            let role: PhotoResourceRole
            switch res.type {
            case .photo, .fullSizePhoto:
                role = .primaryImage
            case .video, .fullSizeVideo:
                role = asset.mediaSubtypes.contains(.photoLive) ? .motionVideo : .primaryVideo
            case .pairedVideo:
                role = .motionVideo
            case .adjustmentData, .adjustmentBasePhoto, .adjustmentBaseVideo:
                role = .auxiliary
            @unknown default:
                role = .auxiliary
            }

            let descriptor = PhotoAssetResourceDescriptor(
                localIdentifier: localIdentifier,
                role: role,
                originalFilename: res.originalFilename,
                uniformTypeIdentifier: res.uniformTypeIdentifier,
                isLocallyAvailable: true,
                fileSizeEstimateBytes: res.value(forKey: "fileSize") as? Int64
            )
            descriptors.append(descriptor)
        }

        return descriptors
    }

    private func classifyAsset(_ asset: PHAsset) -> PhotoAssetKind {
        if asset.mediaSubtypes.contains(.photoLive) {
            return .livePhoto
        }
        if asset.mediaSubtypes.contains(.photoScreenshot) {
            return .screenshot
        }
        switch asset.mediaType {
        case .image:
            return .stillPhoto
        case .video:
            return .video
        default:
            return .unknown
        }
    }
}
