import Foundation
import QuickLook
import SwiftUI
import UIKit
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

    func testWrongNodeLibraryAndVersionCannotAcquireExportCapability() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let wrongNode = try await NodeId.validated(queueUUID(333), using: QueueValidator())
        let wrongLibrary = try await LibraryId.validated(queueUUID(334), using: QueueValidator())
        let wrongVersion = try await FileVersionId.validated(
            queueUUID(335), using: QueueValidator())
        let requestsBefore = await f.content.requests()
        let mismatches = [
            (wrongLibrary, f.node.id, f.version),
            (f.queue.scope.libraryId, wrongNode, f.version),
            (f.queue.scope.libraryId, f.node.id, wrongVersion),
        ]
        for (library, node, version) in mismatches {
            do {
                _ = try await f.coordinator.verifiedFile(
                    library: library, node: node, version: version)
                XCTFail("mismatched immutable scope must not mint an export capability")
            } catch { XCTAssertEqual(error as? DownloadFailure, .stagingMissing) }
        }
        let requestsAfter = await f.content.requests()
        XCTAssertEqual(requestsAfter.count, requestsBefore.count)
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

    func testConcurrentPresentationConsumesLeaseOnlyOnce() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let coordinator = VerifiedFileExportCoordinator(downloads: f.coordinator)
        let prepared = await coordinator.prepare(
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version,
            purpose: .share, displayName: "report.txt")
        let lease = try XCTUnwrap(prepared.lease)

        async let first = try? await coordinator.beginPresentation(lease)
        async let second = try? await coordinator.beginPresentation(lease)
        let (firstURL, secondURL) = await (first, second)
        let successfulURLs = [firstURL, secondURL].compactMap { $0 }
        XCTAssertEqual(successfulURLs.count, 1)
        let url = try XCTUnwrap(successfulURLs.first)
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))
        let released = await coordinator.finish(lease)
        XCTAssertTrue(released)
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
    }

    func testSessionInvalidationRevokesPreparedLeaseBeforeHandoff() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let coordinator = VerifiedFileExportCoordinator(downloads: f.coordinator)
        let prepared = await coordinator.prepare(
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version,
            purpose: .share, displayName: "report.txt")
        let lease = try XCTUnwrap(prepared.lease)
        let url = f.root.deletingLastPathComponent()
            .appendingPathComponent("exports", isDirectory: true)
            .appendingPathComponent(lease.id, isDirectory: true)
            .appendingPathComponent("report.txt")

        coordinator.invalidateSession()
        do {
            _ = try await coordinator.beginPresentation(lease)
            XCTFail("an invalidated lifecycle cannot authorize a new handoff")
        } catch { XCTAssertEqual(error as? DownloadFailure, .staleSession) }
        for _ in 0..<20 where FileManager.default.fileExists(atPath: url.path) {
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
    }

    func testSessionInvalidationRevokesActivePresentation() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let record = try await downloadRecord(f)
        let originalURL = f.root.appendingPathComponent(record.id.rawValue + ".verified")
        let originalBytes = try Data(contentsOf: originalURL)
        let coordinator = VerifiedFileExportCoordinator(downloads: f.coordinator)
        let prepared = await coordinator.prepare(
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version,
            purpose: .share, displayName: "report.txt")
        let lease = try XCTUnwrap(prepared.lease)
        let url = try await coordinator.beginPresentation(lease)
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))

        coordinator.invalidateSession()
        do {
            _ = try await coordinator.beginPresentation(lease)
            XCTFail("an invalidated session cannot authorize another handoff")
        } catch { XCTAssertEqual(error as? DownloadFailure, .staleSession) }
        for _ in 0..<20 where FileManager.default.fileExists(atPath: url.path) {
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
        XCTAssertEqual(try Data(contentsOf: originalURL), originalBytes)
    }

    func testNativePresentersConstructFromVerifiedTemporaryCopy() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let record = try await downloadRecord(f)
        let originalURL = f.root.appendingPathComponent(record.id.rawValue + ".verified")
        let originalBytes = try Data(contentsOf: originalURL)
        let coordinator = VerifiedFileExportCoordinator(downloads: f.coordinator)

        for purpose in [VerifiedFileExportPurpose.preview, .share, .saveToFiles] {
            let prepared = await coordinator.prepare(
                library: f.queue.scope.libraryId, node: f.node.id, version: f.version,
                purpose: purpose, displayName: "report.txt")
            let lease = try XCTUnwrap(prepared.lease)
            let url = try await coordinator.beginPresentation(lease)
            XCTAssertNotEqual(url.standardizedFileURL, originalURL.standardizedFileURL)
            XCTAssertTrue(url.pathComponents.contains("exports"))

            let host = UIHostingController(
                rootView: NativeFilePresenter(
                    presentation: NativeFilePresentation(lease: lease, url: url),
                    onFinished: { _ in }))
            let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 700))
            window.rootViewController = host
            window.makeKeyAndVisible()
            defer {
                window.isHidden = true
                window.rootViewController = nil
            }
            host.loadViewIfNeeded()
            for _ in 0..<5 {
                host.view.setNeedsLayout()
                host.view.layoutIfNeeded()
                try await Task.sleep(for: .milliseconds(20))
            }
            let children = descendants(of: host)

            switch purpose {
            case .preview:
                let preview = try XCTUnwrap(
                    children.compactMap { $0 as? QLPreviewController }.first)
                let source = try XCTUnwrap(preview.dataSource)
                XCTAssertEqual(source.numberOfPreviewItems(in: preview), 1)
                let item = source.previewController(preview, previewItemAt: 0)
                XCTAssertEqual(item.previewItemURL?.standardizedFileURL, url.standardizedFileURL)
            case .share:
                XCTAssertNotNil(children.compactMap { $0 as? UIActivityViewController }.first)
            case .saveToFiles:
                let picker = try XCTUnwrap(
                    children.compactMap { $0 as? UIDocumentPickerViewController }.first)
                XCTAssertNotNil(picker.delegate)
            }

            XCTAssertEqual(try Data(contentsOf: originalURL), originalBytes)
            let released = await coordinator.finish(lease)
            XCTAssertTrue(released)
            host.view.removeFromSuperview()
        }
    }

    func testNativePresenterCancellationCallbacksReleaseTemporaryCopies() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let coordinator = VerifiedFileExportCoordinator(downloads: f.coordinator)

        let cancellations: [(VerifiedFileExportPurpose, NativeFilePresentationResult)] = [
            (.preview, .previewDismissed),
            (.share, .shareCancelled),
            (.saveToFiles, .saveCancelled),
        ]
        for (purpose, expectedResult) in cancellations {
            let prepared = await coordinator.prepare(
                library: f.queue.scope.libraryId, node: f.node.id, version: f.version,
                purpose: purpose, displayName: "report.txt")
            let lease = try XCTUnwrap(prepared.lease)
            let url = try await coordinator.beginPresentation(lease)
            var result: NativeFilePresentationResult?
            var cleanup: Task<Void, Never>?
            let host = UIHostingController(
                rootView: NativeFilePresenter(
                    presentation: NativeFilePresentation(lease: lease, url: url),
                    onFinished: { value in
                        result = value
                        cleanup = Task { _ = await coordinator.finish(lease) }
                    }))
            let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 700))
            window.rootViewController = host
            window.makeKeyAndVisible()
            defer {
                window.isHidden = true
                window.rootViewController = nil
            }
            host.loadViewIfNeeded()
            for _ in 0..<5 {
                host.view.setNeedsLayout()
                host.view.layoutIfNeeded()
                try await Task.sleep(for: .milliseconds(20))
            }
            let children = descendants(of: host)

            switch purpose {
            case .preview:
                let preview = try XCTUnwrap(
                    children.compactMap { $0 as? QLPreviewController }.first)
                let delegate = try XCTUnwrap(preview.delegate)
                delegate.previewControllerDidDismiss?(preview)
            case .share:
                let sheet = try XCTUnwrap(
                    children.compactMap { $0 as? UIActivityViewController }.first)
                sheet.completionWithItemsHandler?(nil, false, nil, nil)
            case .saveToFiles:
                let picker = try XCTUnwrap(
                    children.compactMap { $0 as? UIDocumentPickerViewController }.first)
                let delegate = try XCTUnwrap(picker.delegate)
                delegate.documentPickerWasCancelled?(picker)
            }

            for _ in 0..<50 where result == nil {
                try await Task.sleep(for: .milliseconds(10))
            }
            XCTAssertEqual(result?.message, expectedResult.message)
            if let cleanup { await cleanup.value }
            XCTAssertFalse(FileManager.default.fileExists(atPath: url.path))
            host.view.removeFromSuperview()
        }
    }

    private func descendants(of controller: UIViewController) -> [UIViewController] {
        [controller] + controller.children.flatMap { descendants(of: $0) }
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
