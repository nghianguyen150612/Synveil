import FileProvider
import Foundation
import XCTest

@testable import Synveil

final class FileProviderEnumerationTests: XCTestCase {
    private let domain = try! FileProviderDomainIdentity(
        endpointOrigin: "https://files.example.test",
        accountIdentity: "018f1f5e-7b80-7000-8000-000000000001",
        deviceIdentity: "018f1f5e-7b80-7000-8000-000000000002",
        credentialGeneration: "018f1f5e-7b80-7000-8000-000000000003")
    private let library = "018f1f5e-7b80-7000-8000-000000000010"
    private var directory: URL!
    private var store: FileProviderGenerationStore!
    private var reader: FileProviderMetadataReader {
        FileProviderMetadataReader(store: store, domainID: domain.rawValue)
    }
    private var root: FileProviderItemIdentity {
        try! .libraryRoot(domain: domain, libraryID: library)
    }
    private var folder: FileProviderItemIdentity {
        try! .node(domain: domain, libraryID: library, nodeID: id(20))
    }

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let location = directory!
        store = FileProviderGenerationStore(
            container: FileProviderSharedContainer(resolveContainerURL: { _ in location }),
            capability: FileProviderCapability(
                domainRegistrationEnabled: false,
                metadataPublicationEnabled: true, authenticatedNetworkEnabled: false,
                mutationEnabled: false))
    }
    override func tearDownWithError() throws { try FileManager.default.removeItem(at: directory) }
    private func id(_ suffix: Int) -> String {
        String(format: "018f1f5e-7b80-7000-8000-%012x", suffix)
    }
    private func item(
        _ identity: FileProviderItemIdentity, parent: FileProviderItemIdentity?,
        kind: FileProviderProjectedItemKind, name: String, tombstone: Bool = false,
        revision: String = "1", version: FileProviderVersionIdentity? = nil
    ) -> FileProviderProjectedItem {
        FileProviderProjectedItem(
            identity: identity, parentIdentity: parent, kind: kind,
            displayName: name, byteLength: nil, creationDate: nil, contentModificationDate: nil,
            fileVersionIdentity: version, contentSHA256: nil, metadataRevision: revision,
            isTombstone: tombstone, source: .serverConfirmed)
    }
    private func snapshot(
        _ generation: UInt64, count: Int = 3, name: String = "notes.txt",
        moved: Bool = false, tombstone: Bool = false, omit: Bool = false,
        completeness: FileProviderProjectionCompleteness = .complete
    ) throws -> FileProviderMetadataSnapshot {
        var items = [
            item(root, parent: .domainRoot(domain: domain), kind: .libraryRoot, name: "Work"),
            item(folder, parent: root, kind: .folder, name: "Folder"),
        ]
        if !omit {
            for n in 0..<count {
                let identity = try FileProviderItemIdentity.node(
                    domain: domain, libraryID: library, nodeID: id(100 + n))
                items.append(
                    item(
                        identity, parent: moved ? root : folder, kind: .file,
                        name: n == 0 ? name : "file-\(n)", tombstone: tombstone))
            }
        }
        return FileProviderMetadataSnapshot(
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint, libraryID: library,
            projectionGeneration: generation, completeness: completeness, items: items)
    }
    private func publish(
        _ generation: UInt64, _ snapshots: [FileProviderMetadataSnapshot], authorization: UInt64 = 1
    ) throws {
        try store.withLease(domainID: domain.rawValue, writing: true) {
            try store.commit(
                domain: domain,
                evidence: FileProviderAuthorizationEvidence(
                    domain: domain, state: .authorized, authorizationGeneration: authorization),
                generation: generation, snapshots: snapshots,
                catalog: Set(snapshots.map(\.libraryID)))
        }
    }
    private func anchor(_ container: String = FileProviderMetadataReader.workingSet) throws -> Data
    {
        try reader.withView { try $0.currentAnchor(container: container) }
    }
    private func changes(
        _ anchor: Data, _ container: String = FileProviderMetadataReader.workingSet
    ) throws -> FileProviderChangePage {
        try reader.withView { try $0.changes(container: container, anchor: anchor) }
    }

    func testNativeFolderAndUnknownFileFieldsAreConservative() throws {
        let snapshot = try snapshot(1)
        let folderItem = SynveilFileProviderItem(projected: snapshot.items[1], domain: domain)
        XCTAssertEqual(folderItem.contentType, .folder)
        XCTAssertEqual(folderItem.capabilities, [.allowsContentEnumerating])
        let file = SynveilFileProviderItem(projected: snapshot.items[2], domain: domain)
        XCTAssertEqual(file.contentType, .data)
        XCTAssertTrue(file.capabilities.isEmpty)
        XCTAssertNil(file.documentSize)
        XCTAssertNil(file.creationDate)
        XCTAssertFalse(file.isDownloaded)
        XCTAssertTrue(file.itemVersion.contentVersion.isEmpty)
        XCTAssertEqual(file.itemVersion.metadataVersion.count, 32)
    }
    func testNativeStableIdentityAndSeparateFileVersion() throws {
        let identity = try FileProviderItemIdentity.node(
            domain: domain, libraryID: library, nodeID: id(100))
        let version = try FileProviderVersionIdentity(
            domain: domain, libraryID: library, nodeID: id(100), fileVersionID: id(999))
        let projected = item(
            identity, parent: folder, kind: .file, name: "before", version: version)
        let native = SynveilFileProviderItem(projected: projected, domain: domain)
        XCTAssertEqual(native.itemIdentifier.rawValue, identity.rawValue)
        XCTAssertEqual(native.parentItemIdentifier.rawValue, folder.rawValue)
        XCTAssertEqual(native.itemVersion.contentVersion.count, 32)
        let renamed = SynveilFileProviderItem(
            projected: item(identity, parent: root, kind: .file, name: "after", version: version),
            domain: domain)
        XCTAssertEqual(native.itemIdentifier, renamed.itemIdentifier)
        XCTAssertEqual(native.itemVersion.contentVersion, renamed.itemVersion.contentVersion)
        XCTAssertEqual(renamed.parentItemIdentifier.rawValue, root.rawValue)
    }
    func testNativeLibraryParentIsFrameworkRoot() throws {
        let native = SynveilFileProviderItem(projected: try snapshot(1).items[0], domain: domain)
        XCTAssertEqual(native.parentItemIdentifier, .rootContainer)
    }
    func testRootLibraryFolderAndLookup() throws {
        try publish(1, [snapshot(1)])
        try reader.withView { view in
            XCTAssertEqual(
                try view.page(container: FileProviderMetadataReader.root, token: nil).items.map(
                    \.identity), [root])
            XCTAssertEqual(
                try view.page(container: root.rawValue, token: nil).items.map(\.identity), [folder])
            XCTAssertEqual(try view.page(container: folder.rawValue, token: nil).items.count, 3)
            XCTAssertEqual(try view.lookup(folder.rawValue).displayName, "Folder")
        }
    }
    func testCompleteEmptyCatalogIsValid() throws {
        try publish(1, [])
        XCTAssertTrue(
            try reader.withView {
                try $0.page(container: FileProviderMetadataReader.root, token: nil).items.isEmpty
            })
        XCTAssertEqual(try FileProviderCursor.decode(anchor()).generation, 1)
    }
    func testMissingEvidenceIsErrorInsteadOfEmptyRoot() throws {
        XCTAssertThrowsError(
            try reader.withView {
                try $0.page(container: FileProviderMetadataReader.root, token: nil)
            })
    }
    func testUnknownContainerAndFileAreNotEmptyFolders() throws {
        try publish(1, [snapshot(1)])
        let file = try snapshot(1).items[2].identity.rawValue
        for identifier in ["unknown", file] {
            XCTAssertThrowsError(
                try reader.withView { try $0.page(container: identifier, token: nil) })
        }
    }
    func testWrongLibraryLookupFails() throws {
        try publish(1, [snapshot(1)])
        let other = try FileProviderItemIdentity.node(
            domain: domain, libraryID: id(11), nodeID: id(100))
        XCTAssertThrowsError(try reader.withView { try $0.lookup(other.rawValue) })
    }
    func testTombstonesExcludedAndEmptyFolderIsAuthoritative() throws {
        try publish(1, [snapshot(1, tombstone: true)])
        XCTAssertTrue(
            try reader.withView {
                try $0.page(container: folder.rawValue, token: nil).items.isEmpty
            })
        XCTAssertThrowsError(
            try reader.withView { try $0.lookup(snapshot(1).items[2].identity.rawValue) })
    }
    func testPaginationIsBoundedDeterministicAndComplete() throws {
        try publish(1, [snapshot(1, count: 205)])
        var token: Data?
        var ids: [String] = []
        repeat {
            let batch = try reader.withView {
                try $0.page(container: folder.rawValue, token: token)
            }
            XCTAssertLessThanOrEqual(batch.items.count, 100)
            if let next = batch.next {
                XCTAssertEqual(next.count, 162)
                XCTAssertLessThan(next.count, 500)
            }
            ids += batch.items.map { $0.identity.rawValue }
            token = batch.next
        } while token != nil
        XCTAssertEqual(ids.count, 205)
        XCTAssertEqual(Set(ids).count, 205)
        XCTAssertEqual(ids, ids.sorted())
    }
    func testRepeatedPageAndRetainedGenerationNeverMix() throws {
        try publish(1, [snapshot(1, count: 205)])
        let page = try reader.withView { try $0.page(container: folder.rawValue, token: nil) }
        let first = try reader.withView {
            try $0.page(container: folder.rawValue, token: page.next)
        }
        try publish(2, [snapshot(2, count: 205, name: "renamed")])
        let again = try reader.withView {
            try $0.page(container: folder.rawValue, token: page.next)
        }
        XCTAssertEqual(first.items, again.items)
    }
    func testInvalidCrossContainerAndExpiredPageFail() throws {
        try publish(1, [snapshot(1, count: 205)])
        let token = try XCTUnwrap(
            reader.withView { try $0.page(container: folder.rawValue, token: nil).next })
        XCTAssertThrowsError(
            try reader.withView { try $0.page(container: root.rawValue, token: token) })
        XCTAssertThrowsError(
            try reader.withView { try $0.page(container: folder.rawValue, token: Data([0])) })
        for n in 2...4 { try publish(UInt64(n), [snapshot(UInt64(n), count: 205)]) }
        XCTAssertThrowsError(
            try reader.withView { try $0.page(container: folder.rawValue, token: token) })
    }
    func testAnchorPersistenceMonotonicityAndCorruption() throws {
        try publish(1, [snapshot(1)])
        let first = try anchor()
        XCTAssertEqual(first, try anchor())
        var corrupt = first
        corrupt[10] ^= 1
        XCTAssertThrowsError(try changes(corrupt))
        try publish(2, [snapshot(2, name: "rename")])
        let result = try changes(first)
        XCTAssertEqual(try FileProviderCursor.decode(result.anchor).generation, 2)
        XCTAssertFalse(result.moreComing)
        XCTAssertEqual(try changes(result.anchor).updated, [])
        let cold = FileProviderMetadataReader(store: store, domainID: domain.rawValue)
        XCTAssertEqual(
            try cold.withView {
                try $0.currentAnchor(container: FileProviderMetadataReader.workingSet)
            }, result.anchor)
    }
    func testExpiredAndCrossScopeAnchorsFail() throws {
        try publish(1, [snapshot(1)])
        let first = try anchor()
        XCTAssertThrowsError(try changes(first, folder.rawValue))
        var decoded = try FileProviderCursor.decode(first)
        decoded = FileProviderCursor(
            version: 1, purpose: "anchor", domain: String(repeating: "a", count: 64),
            owner: decoded.owner, authorization: decoded.authorization,
            container: decoded.container,
            generation: decoded.generation, target: decoded.target, offset: decoded.offset)
        XCTAssertThrowsError(try changes(decoded.encoded()))
        for n in 2...4 { try publish(UInt64(n), [snapshot(UInt64(n))]) }
        XCTAssertThrowsError(try changes(first))
    }
    func testAdditionRenameMoveAndDeletionPreserveIdentity() throws {
        try publish(1, [snapshot(1, count: 1)])
        let start = try anchor()
        let folderAnchor = try anchor(folder.rawValue)
        let rootAnchor = try anchor(root.rawValue)
        try publish(2, [snapshot(2, count: 2, name: "renamed", moved: true)])
        let change = try changes(start)
        XCTAssertEqual(change.updated.count, 2)
        XCTAssertTrue(change.deleted.isEmpty)
        XCTAssertEqual(change.updated.first?.identity, try snapshot(1, count: 1).items[2].identity)
        XCTAssertEqual(try changes(folderAnchor, folder.rawValue).updated.count, 1)
        XCTAssertTrue(try changes(folderAnchor, folder.rawValue).deleted.isEmpty)
        XCTAssertEqual(try changes(rootAnchor, root.rawValue).updated.count, 2)
        try publish(3, [snapshot(3, omit: true)])
        XCTAssertEqual(try changes(change.anchor).deleted.count, 2)
    }
    func testTombstoneAndLibraryMembershipRemovalEmitDeletions() throws {
        try publish(1, [snapshot(1)])
        let start = try anchor()
        let catalog = try anchor(FileProviderMetadataReader.root)
        try publish(2, [snapshot(2, tombstone: true)])
        XCTAssertEqual(try changes(start).deleted.count, 3)
        try publish(3, [])
        XCTAssertEqual(
            try changes(catalog, FileProviderMetadataReader.root).deleted, [root.rawValue])
    }
    func testChangeBatchesResumeAcrossRelaunchAndGenerations() throws {
        try publish(1, [snapshot(1, omit: true)])
        var cursor = try anchor(folder.rawValue)
        try publish(2, [snapshot(2, count: 205)])
        var ids: [String] = []
        repeat {
            let result = try changes(cursor, folder.rawValue)
            XCTAssertLessThanOrEqual(result.updated.count + result.deleted.count, 100)
            ids += result.updated.map { $0.identity.rawValue }
            cursor = result.anchor
            if !result.moreComing { break }
        } while ids.count < 300
        XCTAssertEqual(Set(ids).count, 205)
        XCTAssertEqual(ids.count, 205)
        try publish(3, [snapshot(3, count: 205, name: "new")])
        XCTAssertEqual(try changes(cursor, folder.rawValue).updated.count, 1)
    }
    func testPartialPublicationDoesNotDeleteAnything() throws {
        try publish(1, [snapshot(1)])
        let start = try anchor()
        XCTAssertThrowsError(try publish(2, [snapshot(2, completeness: .partial)]))
        XCTAssertTrue(try changes(start).deleted.isEmpty)
        XCTAssertEqual(try anchor(), start)
    }
    func testInterruptedUnitsAndManifestPreservePriorGeneration() throws {
        for point in ["fileDurable", "unitDurable", "beforeCommit"] {
            try publish(1, [snapshot(1)])
            store.fault = { if $0 == point { throw FileProviderSynchronizationError.unavailable } }
            XCTAssertThrowsError(try publish(2, [snapshot(2, name: "new")]))
            store.fault = { _ in }
            XCTAssertEqual(try FileProviderCursor.decode(anchor()).generation, 1)
            try FileManager.default.removeItem(
                at: store.container.generationDirectory(domainID: domain.rawValue))
        }
    }
    func testDuplicateRollbackAndCatalogMismatchAreRejected() throws {
        try publish(2, [snapshot(2)])
        XCTAssertThrowsError(try publish(2, [snapshot(2)]))
        XCTAssertThrowsError(try publish(1, [snapshot(1)]))
        XCTAssertThrowsError(
            try store.withLease(domainID: domain.rawValue, writing: true) {
                try store.commit(
                    domain: domain,
                    evidence: FileProviderAuthorizationEvidence(
                        domain: domain, state: .authorized, authorizationGeneration: 1),
                    generation: 3, snapshots: [snapshot(3)], catalog: [])
            })
    }
    func testCorruptManifestFutureFormatAndSnapshotFailClosed() throws {
        try publish(1, [snapshot(1)])
        let commit = try store.container.url(for: ["generations", domain.rawValue, "commit.json"])
        let original = try Data(contentsOf: commit)
        var json = try XCTUnwrap(JSONSerialization.jsonObject(with: original) as? [String: Any])
        json["schemaVersion"] = 999
        try JSONSerialization.data(withJSONObject: json).write(to: commit)
        XCTAssertThrowsError(try anchor())
        try original.write(to: commit)
        let manifest = try store.readManifest(domainID: domain.rawValue)
        let unit = try XCTUnwrap(manifest.records.first?.libraries.first)
        let unitURL = try store.container.url(for: ["generations", domain.rawValue, unit.filename])
        try Data("corrupt".utf8).write(to: unitURL)
        XCTAssertThrowsError(try anchor())
        try Data([0]).write(to: commit)
        XCTAssertThrowsError(try anchor())
    }
    func testSymlinkInjectionAndQuotaFailClosed() throws {
        try publish(1, [snapshot(1)])
        let manifest = try store.readManifest(domainID: domain.rawValue)
        let unit = try XCTUnwrap(manifest.records.first?.libraries.first)
        let url = try store.container.url(for: ["generations", domain.rawValue, unit.filename])
        try FileManager.default.removeItem(at: url)
        try FileManager.default.createSymbolicLink(
            at: url, withDestinationURL: directory.appendingPathComponent("outside"))
        XCTAssertThrowsError(try anchor())
        XCTAssertThrowsError(
            try store.container.checkGenerationQuota(
                addingBytes: FileProviderSharedContainer.maximumProviderStorageBytes + 1))
    }
    func testRevocationAndAuthorizationReplacementFenceOldAnchors() throws {
        try publish(1, [snapshot(1)])
        let old = try anchor()
        try store.revoke(domainID: domain.rawValue, state: .loggedOut)
        XCTAssertThrowsError(try anchor())
        XCTAssertThrowsError(try publish(2, [snapshot(2)]))
        try publish(2, [snapshot(2)], authorization: 2)
        XCTAssertThrowsError(try changes(old))
        XCTAssertThrowsError(try publish(3, [snapshot(3)], authorization: 1))
    }
    func testFailedReplacementCommitRetainsPreviousGenerationFiles() throws {
        try publish(1, [snapshot(1)])
        store.fault = {
            if $0 == "beforeCommit" { throw FileProviderSynchronizationError.unavailable }
        }
        XCTAssertThrowsError(try publish(2, [snapshot(2)], authorization: 2))
        store.fault = { _ in }
        XCTAssertEqual(try FileProviderCursor.decode(anchor()).generation, 1)
    }
    func testProtectedDataAndProductionGateDenyMetadata() throws {
        try publish(1, [snapshot(1)])
        store.protectedDataAvailable = { false }
        XCTAssertThrowsError(try anchor())
        let production = FileProviderGenerationStore(
            container: store.container, capability: .production)
        XCTAssertThrowsError(try production.readManifest(domainID: domain.rawValue))
    }
    func testSharedReadersAndWriterRevocationUseNonblockingLeases() throws {
        try publish(1, [snapshot(1)])
        try reader.withView { _ in
            XCTAssertNoThrow(try reader.withView { try $0.lookup(folder.rawValue) })
            XCTAssertThrowsError(try store.revoke(domainID: domain.rawValue, state: .revoked))
        }
        try store.revoke(domainID: domain.rawValue, state: .revoked)
        XCTAssertThrowsError(try anchor())
    }
    func testNativeEnumeratorDeliversExactlyOneCompletionAndCancellationError() throws {
        try publish(1, [snapshot(1)])
        let process = FileProviderProcessLifecycle()
        process.launch()
        let enumerator = SynveilFileProviderEnumerator(
            reader: reader, container: folder.rawValue, process: process)
        let observer = EnumerationRecorder()
        enumerator.enumerateItems(
            for: observer,
            startingAt: NSFileProviderPage(NSFileProviderPage.initialPageSortedByName as Data))
        XCTAssertEqual(observer.items.count, 3)
        XCTAssertEqual(observer.completions, 1)
        XCTAssertNil(observer.error)
        enumerator.invalidate()
        let cancelled = EnumerationRecorder()
        enumerator.enumerateItems(
            for: cancelled,
            startingAt: NSFileProviderPage(NSFileProviderPage.initialPageSortedByName as Data))
        XCTAssertEqual(cancelled.completions, 1)
        XCTAssertTrue(cancelled.items.isEmpty)
        XCTAssertEqual((cancelled.error as NSError?)?.code, NSUserCancelledError)
    }
    func testNativeChangeCallbacksAndProcessInvalidation() throws {
        try publish(1, [snapshot(1)])
        let old = try anchor(folder.rawValue)
        try publish(2, [snapshot(2, tombstone: true)])
        let process = FileProviderProcessLifecycle()
        process.launch()
        let enumerator = SynveilFileProviderEnumerator(
            reader: reader, container: folder.rawValue, process: process)
        let observer = ChangeRecorder()
        enumerator.enumerateChanges(for: observer, from: NSFileProviderSyncAnchor(old))
        XCTAssertEqual(observer.deleted.count, 3)
        XCTAssertEqual(observer.completions, 1)
        XCTAssertFalse(observer.moreComing)
        XCTAssertNil(observer.error)
        process.invalidate()
        enumerator.currentSyncAnchor { XCTAssertNil($0) }
    }
    func testErrorMappingDistinguishesAuthorizationMissingItemExpiryAndCorruption() {
        let mappings: [(FileProviderSynchronizationError, NSFileProviderError.Code)] = [
            (.unauthorized, .notAuthenticated), (.noSuchItem, .noSuchItem),
            (.pageExpired, .pageExpired),
            (.anchorExpired, .syncAnchorExpired), (.corrupt, .cannotSynchronize),
            (.unavailable, .serverUnreachable),
        ]
        for (failure, expected) in mappings {
            XCTAssertEqual(FileProviderErrorMapper.map(failure).code, expected.rawValue)
        }
    }

    func testNativeNameAndDatePageOrderingIsPinnedInCursor() throws {
        try publish(1, [snapshot(1, count: 205)])
        let first = try reader.withView {
            try $0.page(container: folder.rawValue, token: nil, ordering: "name")
        }
        XCTAssertEqual(first.items.map(\.displayName), first.items.map(\.displayName).sorted())
        let token = try XCTUnwrap(first.next)
        XCTAssertEqual(try FileProviderCursor.decode(token).purpose, "name")
        let next = try reader.withView {
            try $0.page(container: folder.rawValue, token: token, ordering: "date")
        }
        XCTAssertLessThanOrEqual(
            try XCTUnwrap(first.items.last?.displayName),
            try XCTUnwrap(next.items.first?.displayName))
        let byDate = try reader.withView {
            try $0.page(container: folder.rawValue, token: nil, ordering: "date")
        }
        XCTAssertEqual(try FileProviderCursor.decode(XCTUnwrap(byDate.next)).purpose, "date")
    }

    func testMoveUpdatesRemainLiveRegardlessOfParentCallbackOrder() throws {
        try publish(1, [snapshot(1, count: 1)])
        let oldParent = try anchor(folder.rawValue)
        let newParent = try anchor(root.rawValue)
        try publish(2, [snapshot(2, count: 1, moved: true)])
        let leaving = try changes(oldParent, folder.rawValue)
        let arriving = try changes(newParent, root.rawValue)
        XCTAssertTrue(leaving.deleted.isEmpty)
        XCTAssertTrue(arriving.deleted.isEmpty)
        for batches in [[leaving, arriving], [arriving, leaving]] {
            var simulatedFiles: [String: FileProviderProjectedItem] = [:]
            for batch in batches {
                for item in batch.updated { simulatedFiles[item.identity.rawValue] = item }
                for id in batch.deleted { simulatedFiles[id] = nil }
            }
            XCTAssertEqual(simulatedFiles.count, 1)
            XCTAssertEqual(simulatedFiles.values.first?.parentIdentity, root)
        }
    }

    func testManifestRenameAndDirectorySyncUncertaintyRollsBack() throws {
        try publish(1, [snapshot(1)])
        var manifestStarted = false
        store.fault = { point in
            if point == "manifestWillReplace" { manifestStarted = true }
            if manifestStarted && point == "afterRename" {
                throw FileProviderSynchronizationError.unavailable
            }
        }
        XCTAssertThrowsError(try publish(2, [snapshot(2, name: "new")]))
        store.fault = { _ in }
        XCTAssertEqual(try FileProviderCursor.decode(anchor()).generation, 1)
        try publish(2, [snapshot(2)])
        XCTAssertEqual(try FileProviderCursor.decode(anchor()).generation, 2)
    }

    func testCrashMarkerDeniesReadsUntilWriterRestoresDurableCommit() throws {
        try publish(1, [snapshot(1)])
        let location = try store.container.generationDirectory(domainID: domain.rawValue)
        let data = try Data(contentsOf: location.appendingPathComponent("commit.json"))
        try data.write(to: location.appendingPathComponent("rollback.json"))
        try Data([1]).write(to: location.appendingPathComponent("unsettled"))
        try Data([0]).write(to: location.appendingPathComponent("commit.json"))
        XCTAssertThrowsError(try anchor())
        try publish(2, [snapshot(2)])
        XCTAssertEqual(try FileProviderCursor.decode(anchor()).generation, 2)
    }

    func testInvalidationDuringObserverDeliveryDoesNotAdvanceAnchor() throws {
        try publish(1, [snapshot(1)])
        let old = try anchor(folder.rawValue)
        try publish(2, [snapshot(2, name: "rename")])
        let process = FileProviderProcessLifecycle()
        process.launch()
        let enumerator = SynveilFileProviderEnumerator(
            reader: reader, container: folder.rawValue, process: process)
        let observer = ChangeRecorder()
        observer.onDelivery = { enumerator.invalidate() }
        enumerator.enumerateChanges(for: observer, from: NSFileProviderSyncAnchor(old))
        XCTAssertEqual(observer.completions, 1)
        XCTAssertNotNil(observer.error)
        XCTAssertTrue(observer.deleted.isEmpty)
        let other = SynveilFileProviderEnumerator(
            reader: reader, container: folder.rawValue, process: process)
        let items = EnumerationRecorder()
        items.onDelivery = { process.invalidate() }
        other.enumerateItems(
            for: items,
            startingAt: NSFileProviderPage(NSFileProviderPage.initialPageSortedByName as Data))
        XCTAssertEqual(items.completions, 1)
        XCTAssertNotNil(items.error)
    }

    func testPageCannotReuseLibraryRemovedFromCurrentCatalog() throws {
        try publish(1, [snapshot(1, count: 205)])
        let token = try XCTUnwrap(
            reader.withView { try $0.page(container: folder.rawValue, token: nil).next })
        try publish(2, [])
        XCTAssertThrowsError(
            try reader.withView { try $0.page(container: folder.rawValue, token: token) })
    }

    func testStagedDiskFullFailurePreservesPriorCommit() throws {
        try publish(1, [snapshot(1)])
        store.fault = {
            if $0 == "fileDurable" { throw NSError(domain: NSPOSIXErrorDomain, code: Int(ENOSPC)) }
        }
        XCTAssertThrowsError(try publish(2, [snapshot(2)]))
        store.fault = { _ in }
        XCTAssertEqual(try FileProviderCursor.decode(anchor()).generation, 1)
    }

    private func partitions(_ generation: UInt64, count: Int = 3) throws
        -> [FileProviderContainerSnapshot]
    {
        let first = try snapshot(generation, count: count)
        let rootItem = first.items[0]
        let folderItem = first.items[1]
        return [
            FileProviderContainerSnapshot(
                schemaVersion: 2, domainIdentity: domain.rawValue,
                ownerScopeFingerprint: domain.ownerScopeFingerprint, libraryID: library,
                projectionGeneration: generation, completeness: .complete, ancestors: [rootItem],
                children: [folderItem]),
            FileProviderContainerSnapshot(
                schemaVersion: 2, domainIdentity: domain.rawValue,
                ownerScopeFingerprint: domain.ownerScopeFingerprint, libraryID: library,
                projectionGeneration: generation, completeness: .complete,
                ancestors: [rootItem, folderItem], children: Array(first.items.dropFirst(2))),
        ]
    }

    private func publishPartitions(_ generation: UInt64, units: [FileProviderContainerSnapshot])
        throws
    {
        try store.withLease(domainID: domain.rawValue, writing: true) {
            try store.commit(
                domain: domain,
                evidence: FileProviderAuthorizationEvidence(
                    domain: domain, state: .authorized, authorizationGeneration: 1),
                generation: generation, snapshots: [], catalog: [library], containerSnapshots: units
            )
        }
    }

    func testV2PartitionsAndV1CompatibilityHaveIdenticalEnumeration() throws {
        try publish(1, [snapshot(1)])
        let old = try anchor()
        try publishPartitions(2, units: partitions(2))
        let replay = try changes(old)
        XCTAssertTrue(replay.updated.isEmpty)
        XCTAssertTrue(replay.deleted.isEmpty)
        XCTAssertEqual(
            try reader.withView { try $0.page(container: folder.rawValue, token: nil).items.count },
            3)
    }

    func testV2RejectsMissingFolderListingAndMismatchedAncestorEvidence() throws {
        let units = try partitions(1)
        XCTAssertThrowsError(try publishPartitions(1, units: [units[0]]))
        let wrongAncestor = item(
            root, parent: .domainRoot(domain: domain), kind: .libraryRoot, name: "Other")
        let mismatch = FileProviderContainerSnapshot(
            schemaVersion: 2, domainIdentity: domain.rawValue,
            ownerScopeFingerprint: domain.ownerScopeFingerprint, libraryID: library,
            projectionGeneration: 1,
            completeness: .complete, ancestors: [wrongAncestor, units[1].ancestors[1]],
            children: units[1].children)
        XCTAssertThrowsError(try publishPartitions(1, units: [units[0], mismatch]))
        XCTAssertThrowsError(try publishPartitions(1, units: [units[0], units[1], units[1]]))
    }

    func testLargeLibrarySpansBoundedUnitsBeyondV1SnapshotLimit() throws {
        let baseline = try snapshot(1, count: 5000)
        let rootItem = baseline.items[0]
        let firstFolder = baseline.items[1]
        let secondFolderID = try FileProviderItemIdentity.node(
            domain: domain, libraryID: library, nodeID: id(30))
        let secondFolder = item(secondFolderID, parent: root, kind: .folder, name: "Second")
        let firstChildren = Array(baseline.items.dropFirst(2))
        let secondChildren = try (0..<5000).map { index in
            item(
                try FileProviderItemIdentity.node(
                    domain: domain, libraryID: library, nodeID: id(6000 + index)),
                parent: secondFolderID, kind: .file, name: "second-\(index)")
        }
        let units = [
            ([rootItem], [firstFolder, secondFolder]), ([rootItem, firstFolder], firstChildren),
            ([rootItem, secondFolder], secondChildren),
        ].map { ancestors, children in
            FileProviderContainerSnapshot(
                schemaVersion: 2, domainIdentity: domain.rawValue,
                ownerScopeFingerprint: domain.ownerScopeFingerprint, libraryID: library,
                projectionGeneration: 1, completeness: .complete, ancestors: ancestors,
                children: children)
        }
        XCTAssertTrue(units.allSatisfy { $0.ancestors.count + $0.children.count <= 10_000 })
        try publishPartitions(1, units: units)
        try reader.withView { view in
            XCTAssertEqual(try store.items(view.latest, manifest: view.manifest).count, 10_003)
            XCTAssertEqual(try view.page(container: folder.rawValue, token: nil).items.count, 100)
            XCTAssertEqual(
                try view.page(container: secondFolderID.rawValue, token: nil).items.count, 100)
        }
    }

    func testV2PartialFutureAndOversizedUnitsFailClosed() throws {
        let units = try partitions(1)
        for version in [2, 999] {
            let partial = FileProviderContainerSnapshot(
                schemaVersion: version, domainIdentity: domain.rawValue,
                ownerScopeFingerprint: domain.ownerScopeFingerprint, libraryID: library,
                projectionGeneration: 1, completeness: .partial, ancestors: units[0].ancestors,
                children: units[0].children)
            XCTAssertThrowsError(try publishPartitions(1, units: [partial, units[1]]))
        }
        let oversized = try partitions(1, count: 10_000)
        XCTAssertThrowsError(try publishPartitions(1, units: oversized))
    }

    @MainActor
    func testSignalingCoalescesAndAcknowledgesOnlyCompletedDelivery() async throws {
        try publish(1, [snapshot(1)])
        let manager = SignalRecorder()
        let signals = FileProviderMetadataSignalingCoordinator(store: store, manager: manager)
        try await signals.deliverPending(domainID: domain.rawValue)
        let count = manager.containers.count
        XCTAssertEqual(count, Set(manager.containers).count)
        XCTAssertGreaterThan(count, 0)
        try await signals.deliverPending(domainID: domain.rawValue)
        XCTAssertEqual(manager.containers.count, count)
    }
    @MainActor
    func testFailedAndUnregisteredSignalsStayPending() async throws {
        try publish(1, [snapshot(1)])
        let manager = SignalRecorder()
        let signals = FileProviderMetadataSignalingCoordinator(store: store, manager: manager)
        manager.registered = false
        try await signals.deliverPending(domainID: domain.rawValue)
        XCTAssertFalse(
            try store.signalsAcknowledged(
                domainID: domain.rawValue, manifest: store.readManifest(domainID: domain.rawValue)))
        manager.registered = true
        manager.fail = true
        do {
            try await signals.deliverPending(domainID: domain.rawValue)
            XCTFail("signal failure")
        } catch {}
        manager.fail = false
        try await signals.deliverPending(domainID: domain.rawValue)
        XCTAssertTrue(
            try store.signalsAcknowledged(
                domainID: domain.rawValue, manifest: store.readManifest(domainID: domain.rawValue)))
    }
    @MainActor
    func testHostPublicationRequiresCompleteConfirmedCaptureAndRevalidatesSession() async throws {
        let source = PublicationFixture(
            publication: FileProviderConfirmedPublication(
                domain: domain,
                evidence: FileProviderAuthorizationEvidence(
                    domain: domain, state: .authorized, authorizationGeneration: 1), generation: 1,
                completeCatalog: [library], catalogIsComplete: true,
                snapshots: [try snapshot(1)], serverConfirmedCut: "epoch:7:seq:9",
                locallyAppliedCut: "epoch:7:seq:9",
                containsUnresolvedMutations: false, containsConflictReview: false))
        let coordinator = FileProviderMetadataPublicationCoordinator(store: store, source: source)
        source.valid = false
        do {
            try await coordinator.publishConfirmedRefresh()
            XCTFail("stale source")
        } catch {}
        XCTAssertThrowsError(try anchor())
        source.valid = true
        try await coordinator.publishConfirmedRefresh()
        XCTAssertEqual(try FileProviderCursor.decode(anchor()).generation, 1)
        try coordinator.withdraw(domainID: domain.rawValue, state: .accountSwitched)
        XCTAssertThrowsError(try anchor())
    }
}

private final class EnumerationRecorder: NSObject, NSFileProviderEnumerationObserver {
    var items: [NSFileProviderItem] = []
    var completions = 0
    var error: Error?
    var onDelivery: (() -> Void)?
    func didEnumerate(_ updatedItems: [NSFileProviderItem]) {
        items += updatedItems
        onDelivery?()
    }
    func finishEnumerating(upTo nextPage: NSFileProviderPage?) { completions += 1 }
    func finishEnumeratingWithError(_ error: Error) {
        completions += 1
        self.error = error
    }
}
private final class ChangeRecorder: NSObject, NSFileProviderChangeObserver {
    var deleted: [NSFileProviderItemIdentifier] = []
    var completions = 0
    var moreComing = false
    var error: Error?
    var onDelivery: (() -> Void)?
    func didUpdate(_ updatedItems: [NSFileProviderItem]) { onDelivery?() }
    func didDeleteItems(withIdentifiers identifiers: [NSFileProviderItemIdentifier]) {
        deleted += identifiers
    }
    func finishEnumeratingChanges(upTo anchor: NSFileProviderSyncAnchor, moreComing: Bool) {
        completions += 1
        self.moreComing = moreComing
    }
    func finishEnumeratingWithError(_ error: Error) {
        completions += 1
        self.error = error
    }
}
@MainActor
private final class SignalRecorder: FileProviderMetadataSignaling {
    var containers: [String] = []
    var registered = true
    var fail = false
    func signal(domainID: String, container: String) async throws -> Bool {
        containers.append(container)
        if fail { throw FileProviderSynchronizationError.unavailable }
        return registered
    }
}
@MainActor
private final class PublicationFixture: FileProviderPublicationSource {
    let publication: FileProviderConfirmedPublication
    var valid = true
    init(publication: FileProviderConfirmedPublication) { self.publication = publication }
    func captureConfirmedPublication() async throws -> FileProviderConfirmedPublication {
        publication
    }
    func validateForCommit(_ publication: FileProviderConfirmedPublication) throws {
        if !valid { throw FileProviderSynchronizationError.unauthorized }
    }
}
