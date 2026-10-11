import Foundation
import XCTest

@testable import Synveil

@MainActor
final class VerifiedFileExportTests: XCTestCase {
    func testLocalCapabilityAcquisitionCreatesNoContentRequest() async throws {
        let f = try await downloadFixture(self)
        do {
            _ = try await f.coordinator.verifiedFile(
                library: f.queue.scope.libraryId, node: f.node.id, version: f.version)
            XCTFail("there is no verified record yet")
        } catch { XCTAssertEqual(error as? DownloadFailure, .stagingMissing) }
        let requestsBefore = await f.content.requests()
        XCTAssertTrue(requestsBefore.isEmpty)
        _ = try downloadCapability(await downloadRun(f))
        let count = await f.content.requests().count
        let metadataCount = await f.metadata.requests().count
        let reopened = try await f.coordinator.verifiedFile(
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version)
        XCTAssertEqual(reopened.identity.versionId, f.version)
        let requestsAfter = await f.content.requests()
        XCTAssertEqual(requestsAfter.count, count)
        let metadataAfter = await f.metadata.requests()
        XCTAssertEqual(metadataAfter.count, metadataCount)
    }

    func testPartialRecordCannotAcquireExportCapability() async throws {
        let f = try await downloadFixture(self, body: Data(repeating: 0x61, count: 200_000))
        await f.content.configure(failAfterChunks: 1)
        guard case .stopped = await downloadRun(f) else {
            return XCTFail("fixture must remain partial")
        }
        do {
            _ = try await f.coordinator.verifiedFile(
                library: f.queue.scope.libraryId, node: f.node.id, version: f.version)
            XCTFail("partial bytes cannot create a verified capability")
        } catch { XCTAssertEqual(error as? DownloadFailure, .stagingMissing) }
    }

    func testBoundedTemporaryCopyPreservesVerifiedBytesAndUsesSafeName() async throws {
        let body = Data(repeating: 0x61, count: 16 * 1024 * 1024)
        let f = try await downloadFixture(self, body: body)
        _ = try downloadCapability(await downloadRun(f))
        let record = try await downloadRecord(f)
        let id = UUID().uuidString.lowercased()
        try await f.staging.reserveExport(
            id: id, bytes: record.identity.expected.byteLength,
            durable: try await f.queue.database.downloadReservations())
        let url = try await f.staging.copyVerifiedForExport(
            record, exportId: id, displayName: "../../合同\nfile.txt")
        XCTAssertEqual(url.lastPathComponent, "_.._合同_file.txt")
        let fileAttributes = try FileManager.default.attributesOfItem(atPath: url.path)
        XCTAssertEqual(fileAttributes[.size] as? NSNumber, NSNumber(value: body.count))
        XCTAssertEqual(try Data(contentsOf: url), f.body)
        let checked = try await f.staging.verifyExport(
            id: id, displayName: "../../合同\nfile.txt", expected: record.identity.expected)
        XCTAssertEqual(checked, url)
        try await f.staging.removeExport(id)
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: f.root.appendingPathComponent(record.id.rawValue + ".verified").path))
    }

    func testTamperedTemporaryCopyCannotBeVerified() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let record = try await downloadRecord(f)
        let id = UUID().uuidString.lowercased()
        try await f.staging.reserveExport(
            id: id, bytes: record.identity.expected.byteLength,
            durable: try await f.queue.database.downloadReservations())
        let url = try await f.staging.copyVerifiedForExport(
            record, exportId: id, displayName: "note.txt")
        try Data("xxxxx".utf8).write(to: url)
        do {
            _ = try await f.staging.verifyExport(
                id: id, displayName: "note.txt", expected: record.identity.expected)
            XCTFail("modified presentation bytes must fail verification")
        } catch { XCTAssertEqual(error as? DownloadFailure, .sha256Mismatch) }
        try await f.staging.removeExport(id)
    }

    func testTemporaryCopyReservationSharesDownloadQuota() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let record = try await downloadRecord(f)
        do {
            try await f.staging.reserveExport(
                id: UUID().uuidString.lowercased(), bytes: DownloadPolicy.quota,
                durable: try await f.queue.database.downloadReservations())
            XCTFail("source plus a full quota sized copy must exceed the shared limit")
        } catch { XCTAssertEqual(error as? DownloadFailure, .stagingQuotaExceeded) }
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: f.root.appendingPathComponent(record.id.rawValue + ".verified").path))
    }

    func testCrashOrphanCleanupRemovesOnlyTemporaryCopy() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let record = try await downloadRecord(f)
        let id = UUID().uuidString.lowercased()
        try await f.staging.reserveExport(
            id: id, bytes: record.identity.expected.byteLength,
            durable: try await f.queue.database.downloadReservations())
        let url = try await f.staging.copyVerifiedForExport(
            record, exportId: id, displayName: "report.txt")
        let reopened = try DownloadStagingStore(root: f.root)
        try await reopened.cleanupOrphanExports()
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: f.root.appendingPathComponent(record.id.rawValue + ".verified").path))
    }

    func testFilenameSanitizationBlocksPathAndControlCharacters() {
        XCTAssertEqual(
            SafeExportFilename.make("../../secret\\name\u{0000}.pdf"),
            "_.._secret_name_.pdf")
        XCTAssertEqual(SafeExportFilename.make("..."), "Synveil Download")
        let longName = SafeExportFilename.make(String(repeating: "界", count: 400))
        XCTAssertLessThanOrEqual(longName.utf8.count, 180)
        XCTAssertEqual(SafeExportFilename.make("résumé.txt"), "résumé.txt")
        XCTAssertEqual(SafeExportFilename.make(".txt"), "Synveil Download.txt")
    }

    func testForgedAndReusedLeasesAreRejected() async throws {
        let f = try await downloadFixture(self)
        let capability = try downloadCapability(await downloadRun(f))
        let coordinator = VerifiedFileExportCoordinator(downloads: f.coordinator)
        let forged = VerifiedFileExportLease(
            id: UUID().uuidString.lowercased(), purpose: .share,
            sourceOperationId: capability.operationId, versionId: f.version,
            sha256: capability.identity.expected.sha256,
            byteLength: capability.identity.expected.byteLength,
            lifecycleRevision: capability.session.lifecycleRevision)
        do {
            _ = try await coordinator.beginPresentation(forged)
            XCTFail("forged leases are not registered for handoff")
        } catch { XCTAssertEqual(error as? DownloadFailure, .staleSession) }
        let prepared = await coordinator.prepare(
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version,
            purpose: .share, displayName: f.node.name)
        let lease = try XCTUnwrap(prepared.lease)
        let url = try await coordinator.beginPresentation(lease)
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))
        let released = await coordinator.finish(lease)
        XCTAssertTrue(released)
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
        do {
            _ = try await coordinator.beginPresentation(lease)
            XCTFail("released leases cannot be reused")
        } catch { XCTAssertEqual(error as? DownloadFailure, .staleSession) }
    }

    func testPreviewRequiresConservativeTypeAndSignature() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let pdf = root.appendingPathComponent("report.pdf")
        try Data("%PDF-1.7".utf8).write(to: pdf)
        XCTAssertTrue(
            VerifiedFileExportCoordinator.supportsPreview(url: pdf, displayName: "report.pdf"))
        let mismatch = root.appendingPathComponent("report.png")
        try Data("not an image".utf8).write(to: mismatch)
        XCTAssertFalse(
            VerifiedFileExportCoordinator.supportsPreview(
                url: mismatch, displayName: "report.png"))
        let html = root.appendingPathComponent("report.html")
        try Data("<html><script>alert(1)</script></html>".utf8).write(to: html)
        XCTAssertFalse(
            VerifiedFileExportCoordinator.supportsPreview(url: html, displayName: "report.html"))
    }
}
