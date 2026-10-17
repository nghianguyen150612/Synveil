import Foundation

/// PhotoKit-only staging boundary backed by the shared P043/P045 staging root and quota.
/// The opaque `.photo` source is verified before it is handed to the P045 `.upload` importer.
actor PhotoResourceStagingCoordinator {
    private let uploadStaging: UploadStagingStore

    init(uploadStaging: UploadStagingStore) {
        self.uploadStaging = uploadStaging
    }

    func begin(
        stagingId: UploadOperationId,
        maximumBytes: UInt64,
        durableReservations: [DownloadStorageReservation]
    ) async throws -> PhotoResourceStageWriter {
        try await uploadStaging.createPhotoResourceStage(
            stagingId: stagingId, maximumBytes: maximumBytes,
            durableReservations: durableReservations)
    }

    func publish(
        stagingId: UploadOperationId,
        writer: PhotoResourceStageWriter
    ) async throws -> PhotoResourceVerifiedContent {
        try await uploadStaging.publishPhotoResourceStage(stagingId: stagingId, writer: writer)
    }

    func verify(
        stagingId: String, byteCount: UInt64, sha256: String
    ) async throws -> PhotoResourceVerifiedContent {
        try await uploadStaging.verifyPhotoResourceStage(
            stagingId: stagingId, expectedByteCount: byteCount, expectedSHA256: sha256)
    }

    func discard(stagingId: String) async {
        await uploadStaging.discardPhotoResourceStage(stagingId: stagingId)
    }

    func recoverAfterLaunch(keeping stagingIds: Set<String>) async throws {
        try await uploadStaging.reconcileOrphanPhotoResources(keeping: stagingIds)
    }
}
