import Foundation
import Photos

public final class PhotoKitResourceExtractionSource: PhotoResourceExtractionSource,
    @unchecked Sendable
{
    private let permissionAdapter: PhotoLibraryPermissionChecking
    private let manager: PHAssetResourceManager

    public init(
        permissionAdapter: PhotoLibraryPermissionChecking = PhotoLibraryPermissionAdapter(),
        manager: PHAssetResourceManager = .default()
    ) {
        self.permissionAdapter = permissionAdapter
        self.manager = manager
    }

    public func selections(
        for localIdentifier: String,
        includingEditedRenditions: Bool
    ) async throws -> [PhotoResourceSelection] {
        guard await permissionAdapter.currentPermissionState().isAuthorized else {
            throw PhotoResourceTransferFailure.permissionDenied
        }
        guard let asset = fetchAsset(localIdentifier) else {
            throw PhotoResourceTransferFailure.sourceUnavailable
        }
        return PHAssetResource.assetResources(for: asset).enumerated().compactMap {
            index, resource in
            let (role, rendition) = PhotoLibraryDiscoveryService.classifyResource(
                resource.type, asset: asset)
            guard includingEditedRenditions || rendition != .edited else { return nil }
            return PhotoResourceSelection(
                localIdentifier: localIdentifier,
                resourceIndex: index,
                resourceType: String(resource.type.rawValue),
                role: role,
                rendition: rendition,
                uniformTypeIdentifier: resource.uniformTypeIdentifier,
                sourceModificationDate: asset.modificationDate)
        }
    }

    public func currentModificationDate(for localIdentifier: String) async throws -> Date? {
        guard await permissionAdapter.currentPermissionState().isAuthorized else {
            throw PhotoResourceTransferFailure.permissionDenied
        }
        guard let asset = fetchAsset(localIdentifier) else {
            throw PhotoResourceTransferFailure.sourceUnavailable
        }
        return asset.modificationDate
    }

    public func extract(
        _ selection: PhotoResourceSelection,
        maximumBytes: UInt64,
        receive: @escaping @Sendable (Data) throws -> Void
    ) async throws -> UInt64 {
        guard maximumBytes > 0,
            await permissionAdapter.currentPermissionState().isAuthorized
        else { throw PhotoResourceTransferFailure.permissionDenied }
        guard let asset = fetchAsset(selection.localIdentifier) else {
            throw PhotoResourceTransferFailure.sourceUnavailable
        }
        guard photoTimestampValuesMatch(asset.modificationDate, selection.sourceModificationDate)
        else {
            throw PhotoResourceTransferFailure.sourceChanged
        }
        let resources = PHAssetResource.assetResources(for: asset)
        guard resources.indices.contains(selection.resourceIndex) else {
            throw PhotoResourceTransferFailure.sourceUnavailable
        }
        let resource = resources[selection.resourceIndex]
        guard PhotoLibraryDiscoveryService.isSupportedResourceType(resource.type) else {
            throw PhotoResourceTransferFailure.unsupportedResource
        }
        let mapping = PhotoLibraryDiscoveryService.classifyResource(resource.type, asset: asset)
        guard String(resource.type.rawValue) == selection.resourceType,
            mapping.0 == selection.role, mapping.1 == selection.rendition,
            resource.uniformTypeIdentifier == selection.uniformTypeIdentifier
        else { throw PhotoResourceTransferFailure.sourceChanged }

        let options = PHAssetResourceRequestOptions()
        options.isNetworkAccessAllowed = false
        let request = PhotoResourceRequestState()
        let budget = PhotoResourceReceiveBudget(maximumBytes: maximumBytes)
        let manager = self.manager
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                let requestId = manager.requestData(
                    for: resource,
                    options: options,
                    dataReceivedHandler: { data in
                        do {
                            guard !request.isFinished else { return }
                            guard !request.isCancelled else { throw CancellationError() }
                            guard !data.isEmpty else { return }
                            try budget.accept(data)
                            var offset = 0
                            while offset < data.count {
                                guard !request.isCancelled else { throw CancellationError() }
                                let end = min(offset + 256 * 1024, data.count)
                                try receive(Data(data[offset..<end]))
                                offset = end
                            }
                        } catch {
                            request.recordFailure(error)
                            request.cancel(using: manager)
                        }
                    },
                    completionHandler: { error in
                        guard request.finishOnce() else { return }
                        if let failure = request.failure ?? error {
                            if failure is CancellationError || request.isCancelled {
                                continuation.resume(throwing: CancellationError())
                            } else if let transferFailure = failure
                                as? PhotoResourceTransferFailure
                            {
                                continuation.resume(throwing: transferFailure)
                            } else {
                                // With iCloud access disabled, PhotoKit reports a missing local
                                // original here. Never request a preview or silently fetch it.
                                continuation.resume(
                                    throwing: PhotoResourceTransferFailure.sourceUnavailable)
                            }
                            return
                        }
                        continuation.resume(returning: budget.byteCount)
                    })
                request.install(requestId: requestId, manager: manager)
            }
        } onCancel: {
            request.cancel(using: manager)
        }
    }

    private func fetchAsset(_ localIdentifier: String) -> PHAsset? {
        PHAsset.fetchAssets(withLocalIdentifiers: [localIdentifier], options: nil).firstObject
    }
}

private final class PhotoResourceRequestState: @unchecked Sendable {
    private let lock = NSLock()
    private var requestId: PHAssetResourceDataRequestID?
    private var cancelled = false
    private var finished = false
    private var storedFailure: Error?

    var isCancelled: Bool {
        lock.lock()
        defer { lock.unlock() }
        return cancelled
    }

    var isFinished: Bool {
        lock.lock()
        defer { lock.unlock() }
        return finished
    }

    var failure: Error? {
        lock.lock()
        defer { lock.unlock() }
        return storedFailure
    }

    func install(requestId: PHAssetResourceDataRequestID, manager: PHAssetResourceManager) {
        lock.lock()
        self.requestId = requestId
        let shouldCancel = cancelled && !finished
        lock.unlock()
        if shouldCancel { manager.cancelDataRequest(requestId) }
    }

    func recordFailure(_ error: Error) {
        lock.lock()
        if storedFailure == nil { storedFailure = error }
        lock.unlock()
    }

    func finishOnce() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard !finished else { return false }
        finished = true
        return true
    }

    func cancel(using manager: PHAssetResourceManager) {
        lock.lock()
        cancelled = true
        let requestId = self.requestId
        let shouldCancel = !finished
        lock.unlock()
        if shouldCancel, let requestId { manager.cancelDataRequest(requestId) }
    }
}

private final class PhotoResourceReceiveBudget: @unchecked Sendable {
    private let lock = NSLock()
    private let maximumBytes: UInt64
    private var received: UInt64 = 0
    private let maximumChunkBytes = 1024 * 1024

    init(maximumBytes: UInt64) { self.maximumBytes = maximumBytes }

    var byteCount: UInt64 {
        lock.lock()
        defer { lock.unlock() }
        return received
    }

    func accept(_ data: Data) throws {
        lock.lock()
        defer { lock.unlock() }
        guard data.count <= maximumChunkBytes else {
            throw PhotoResourceTransferFailure.resourceTooLarge
        }
        let next = received.addingReportingOverflow(UInt64(data.count))
        guard !next.overflow, next.partialValue <= maximumBytes else {
            throw PhotoResourceTransferFailure.resourceTooLarge
        }
        received = next.partialValue
    }
}
