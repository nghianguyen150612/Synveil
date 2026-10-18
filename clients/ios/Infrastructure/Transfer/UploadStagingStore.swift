import Foundation

/// Upload-only access to the shared private transfer store. Quota is owned by the download
/// staging actor, which inventories uploads, downloads and export copies under one ceiling.
actor UploadStagingStore {
    private let shared: DownloadStagingStore

    init(shared: DownloadStagingStore) { self.shared = shared }

    func importSelectedFile(
        _ selectedURL: URL, stagingId: UploadOperationId,
        durableReservations: [DownloadStorageReservation]
    ) async throws -> UploadExpectedContent {
        try await shared.stageUpload(
            from: selectedURL, stagingId: stagingId, durable: durableReservations)
    }

    func createPhotoResourceStage(
        stagingId: UploadOperationId, maximumBytes: UInt64,
        durableReservations: [DownloadStorageReservation]
    ) async throws -> PhotoResourceStageWriter {
        try await shared.createPhotoResourceStage(
            id: stagingId, maximumBytes: maximumBytes, durable: durableReservations)
    }

    func publishPhotoResourceStage(
        stagingId: UploadOperationId, writer: PhotoResourceStageWriter
    ) async throws -> PhotoResourceVerifiedContent {
        try await shared.publishPhotoResourceStage(id: stagingId, writer: writer)
    }

    func verifyPhotoResourceStage(
        stagingId: String, expectedByteCount: UInt64, expectedSHA256: String
    ) async throws -> PhotoResourceVerifiedContent {
        try await shared.verifyPhotoResourceStage(
            id: stagingId, expectedByteCount: expectedByteCount, expectedSHA256: expectedSHA256)
    }

    func discardPhotoResourceStage(stagingId: String) async {
        try? await shared.discardPhotoResourceStage(id: stagingId)
    }

    func reconcileOrphanPhotoResources(keeping stagingIds: Set<String>) async throws {
        try await shared.reconcileOrphanPhotoResources(keeping: stagingIds)
    }

    func verify(_ record: UploadRecord) async throws {
        guard !record.stagingReleased else { throw UploadFailure.stagingMissing }
        try await shared.verifyUploadSource(
            stagingId: record.stagingId, expected: record.intent.expected)
    }

    func chunk(_ record: UploadRecord, offset: UInt64, count: Int) async throws -> Data {
        guard !record.stagingReleased else { throw UploadFailure.stagingMissing }
        return try await shared.uploadChunk(
            stagingId: record.stagingId, expected: record.intent.expected,
            offset: offset, count: count)
    }

    func prepareBackgroundChunk(
        _ record: UploadRecord, id: UploadOperationId,
        offset: UInt64, count: Int, durable: [DownloadStorageReservation]
    ) async throws -> BackgroundChunkIntent {
        try await shared.prepareBackgroundChunk(
            record, id: id, offset: offset, count: count, durable: durable)
    }

    func backgroundChunkURL(_ chunk: BackgroundChunkIntent) async throws -> URL {
        try await shared.backgroundChunkURL(chunk)
    }

    func removeBackgroundChunk(_ chunk: BackgroundChunkIntent) async throws {
        try await shared.removeBackgroundChunk(chunk)
    }

    func removeCommittedSource(_ record: UploadRecord) async throws {
        guard record.state == .committed, record.completion != nil else {
            throw UploadFailure.invalidMetadata
        }
        try await shared.removeUploadSource(stagingId: record.stagingId)
    }

    func discardUncommitted(_ stagingId: UploadOperationId) async {
        try? await shared.removeUploadSource(stagingId: stagingId)
    }

    func cancelReservation(_ stagingId: UploadOperationId) async {
        await shared.cancelUploadReservation(stagingId: stagingId)
    }

    func reconcileOrphans(keeping stagingIds: Set<String>) async throws {
        try await shared.reconcileOrphanUploads(keeping: stagingIds)
    }
}
