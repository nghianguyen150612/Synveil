import Foundation
import Photos

public final class PhotoLibraryDiscoveryService: PhotoLibraryDiscoveryProtocol,
    @unchecked Sendable
{
    private struct Snapshot {
        let generation: Int64
        let fetchResult: PHFetchResult<PHAsset>
        let permission: PhotoPermissionState
    }

    private let permissionAdapter: PhotoLibraryPermissionChecking
    private let snapshotLock = NSLock()
    private var snapshots: [UUID: Snapshot] = [:]

    public init(
        permissionAdapter: PhotoLibraryPermissionChecking = PhotoLibraryPermissionAdapter()
    ) {
        self.permissionAdapter = permissionAdapter
    }

    public func discoverAssets(
        configuration: PhotoBackupConfiguration,
        generation: Int64,
        batchSize: Int = 500
    ) async throws -> PhotoDiscoveryBatchResult {
        try await discoverAssetPage(
            configuration: configuration, generation: generation, batchSize: batchSize,
            continuation: nil)
    }

    public func discoverAssetPage(
        configuration: PhotoBackupConfiguration,
        generation: Int64,
        batchSize: Int,
        continuation: PhotoDiscoveryContinuation?
    ) async throws -> PhotoDiscoveryBatchResult {
        guard configuration.isOptedIn && !configuration.pauseDiscovery else {
            return PhotoDiscoveryBatchResult(observations: [], isComplete: true, totalInScope: 0)
        }

        let permission = await permissionAdapter.currentPermissionState()
        guard permission.isAuthorized else {
            return PhotoDiscoveryBatchResult(observations: [], isComplete: false, totalInScope: 0)
        }

        let snapshotId: UUID
        let snapshot: Snapshot
        if let continuation {
            snapshotLock.lock()
            let storedSnapshot = snapshots[continuation.snapshotId]
            snapshotLock.unlock()
            guard continuation.generation == generation,
                continuation.nextOffset >= 0,
                let stored = storedSnapshot,
                stored.generation == generation
            else { throw PhotoDiscoveryFailure.continuationExpired }
            snapshotId = continuation.snapshotId
            snapshot = stored
        } else {
            let fetchResult = PHAsset.fetchAssets(with: fetchOptions(configuration))
            snapshotId = UUID()
            snapshot = Snapshot(
                generation: generation, fetchResult: fetchResult, permission: permission)
            snapshotLock.lock()
            snapshots.removeAll(keepingCapacity: false)
            snapshots[snapshotId] = snapshot
            snapshotLock.unlock()
        }

        guard snapshot.permission == permission else {
            throw PhotoDiscoveryFailure.permissionChanged
        }
        let fetchResult = snapshot.fetchResult
        let totalCount = fetchResult.count
        let requestedBatchSize = min(max(batchSize, 1), 1_000)
        let startOffset = continuation?.nextOffset ?? 0
        guard startOffset <= totalCount,
            continuation.map({ $0.totalInScope == totalCount }) ?? true
        else { throw PhotoDiscoveryFailure.continuationExpired }
        let pageCount = min(requestedBatchSize, totalCount - startOffset)
        let endOffset = startOffset + pageCount
        let isComplete = endOffset == totalCount

        var observations: [PhotoAssetObservation] = []
        observations.reserveCapacity(endOffset - startOffset)
        let now = Date()

        for idx in startOffset..<endOffset {
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

        let next: PhotoDiscoveryContinuation?
        if isComplete {
            snapshotLock.lock()
            snapshots[snapshotId] = nil
            snapshotLock.unlock()
            next = nil
        } else {
            next = PhotoDiscoveryContinuation(
                generation: generation, snapshotId: snapshotId,
                nextOffset: endOffset, totalInScope: totalCount)
        }

        return PhotoDiscoveryBatchResult(
            observations: observations,
            isComplete: isComplete,
            totalInScope: totalCount,
            continuation: next
        )
    }

    public func invalidateDiscoverySnapshots() {
        snapshotLock.lock()
        snapshots.removeAll(keepingCapacity: false)
        snapshotLock.unlock()
    }

    private func fetchOptions(_ configuration: PhotoBackupConfiguration) -> PHFetchOptions {
        let options = PHFetchOptions()
        options.sortDescriptors = [
            NSSortDescriptor(key: "creationDate", ascending: true),
            NSSortDescriptor(key: "localIdentifier", ascending: true),
        ]
        if !configuration.includeVideos {
            options.predicate = NSPredicate(
                format: "mediaType == %d", PHAssetMediaType.image.rawValue)
        }
        return options
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

        for (resourceIndex, res) in resources.enumerated() {
            let (role, rendition) = Self.classifyResource(res.type, asset: asset)
            let descriptor = PhotoAssetResourceDescriptor(
                localIdentifier: localIdentifier,
                role: role,
                resourceIndex: resourceIndex,
                resourceType: String(res.type.rawValue),
                rendition: rendition,
                originalFilename: res.originalFilename,
                uniformTypeIdentifier: res.uniformTypeIdentifier,
                availability: .unknown,
                fileSizeEstimateBytes: nil
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

    static func classifyResource(
        _ type: PHAssetResourceType, asset: PHAsset
    ) -> (PhotoResourceRole, PhotoResourceRendition) {
        switch type {
        case .photo, .adjustmentBasePhoto:
            return (.primaryImage, .original)
        case .alternatePhoto:
            return (.auxiliary, .auxiliary)
        case .fullSizePhoto:
            return (.primaryImage, .edited)
        case .video, .adjustmentBaseVideo:
            return (
                asset.mediaSubtypes.contains(.photoLive) ? .motionVideo : .primaryVideo,
                .original
            )
        case .fullSizeVideo:
            return (
                asset.mediaSubtypes.contains(.photoLive) ? .motionVideo : .primaryVideo,
                .edited
            )
        case .pairedVideo, .adjustmentBasePairedVideo:
            return (.motionVideo, .pairedMotion)
        case .fullSizePairedVideo:
            return (.motionVideo, .edited)
        case .photoProxy, .adjustmentData, .audio:
            return (.auxiliary, .auxiliary)
        @unknown default:
            return (.auxiliary, .auxiliary)
        }
    }

    static func isSupportedResourceType(_ type: PHAssetResourceType) -> Bool {
        switch type {
        case .photo, .fullSizePhoto, .alternatePhoto, .photoProxy, .video, .fullSizeVideo,
            .pairedVideo, .fullSizePairedVideo, .adjustmentBasePairedVideo, .adjustmentData,
            .adjustmentBasePhoto, .adjustmentBaseVideo, .audio:
            return true
        @unknown default:
            return false
        }
    }
}
