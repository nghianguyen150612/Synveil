import Darwin
import Foundation

/// One host writer, immutable Library units and a single atomic commit manifest. Readers hold a
/// short nonblocking shared lease through delivery; revocation takes the exclusive lease. No
/// callback can deliver old evidence after a successful revocation, even if it loaded it earlier.
struct FileProviderGenerationStore {
    let container: FileProviderSharedContainer
    let capability: FileProviderCapability
    var protectedDataAvailable: () -> Bool = { true }
    var fault: (String) throws -> Void = { _ in }

    func withLease<T>(domainID: String, writing: Bool, _ body: () throws -> T) throws -> T {
        let directory = try container.generationDirectory(domainID: domainID)
        let lockURL = directory.appendingPathComponent("lease")
        let fd = open(lockURL.path, O_RDWR | O_CREAT | O_NOFOLLOW, 0o600)
        guard fd >= 0 else { throw FileProviderSynchronizationError.unavailable }
        defer { close(fd) }
        guard flock(fd, (writing ? LOCK_EX : LOCK_SH) | LOCK_NB) == 0 else {
            throw FileProviderSynchronizationError.busy
        }
        defer { flock(fd, LOCK_UN) }
        return try body()
    }

    func readManifest(domainID: String) throws -> FileProviderGenerationManifest {
        guard capability.metadataPublicationEnabled, protectedDataAvailable() else {
            throw FileProviderSynchronizationError.unauthorized
        }
        let url = try container.url(for: ["generations", domainID, "commit.json"])
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw FileProviderSynchronizationError.unauthorized
        }
        let data = try readFile(url, limit: FileProviderSharedContainer.maximumSnapshotBytes)
        let manifest: FileProviderGenerationManifest
        do {
            manifest = try JSONDecoder().decode(FileProviderGenerationManifest.self, from: data)
        } catch { throw FileProviderSynchronizationError.corrupt }
        guard manifest.schemaVersion == FileProviderGenerationManifest.format else {
            throw FileProviderSynchronizationError.incompatible
        }
        guard manifest.domain.rawValue == domainID,
            case .permittedForMetadataRead = FileProviderAuthorizationBoundary.evaluate(
                evidence: manifest.authorization, expectedDomain: manifest.domain,
                protectedDataAvailable: protectedDataAvailable(), capability: capability)
        else { throw FileProviderSynchronizationError.unauthorized }
        guard !manifest.records.isEmpty,
            manifest.records.count <= FileProviderGenerationManifest.retainedGenerations,
            manifest.records.allSatisfy({ $0.generation > 0 && $0.libraries.count <= 128 }),
            zip(manifest.records, manifest.records.dropFirst()).allSatisfy({
                $0.generation < $1.generation
            })
        else { throw FileProviderSynchronizationError.corrupt }
        return manifest
    }

    func snapshots(
        _ record: FileProviderGenerationRecord, manifest: FileProviderGenerationManifest
    ) throws -> [FileProviderMetadataSnapshot] {
        var libraries: Set<String> = []
        return try record.libraries.map { unit in
            guard libraries.insert(unit.libraryID).inserted,
                unit.filename.hasSuffix(".json"),
                FileProviderSharedContainer.isSafePathComponent(unit.filename)
            else { throw FileProviderSynchronizationError.corrupt }
            let url = try container.url(for: [
                "generations", manifest.domain.rawValue, unit.filename,
            ])
            let data = try readFile(url, limit: FileProviderSharedContainer.maximumSnapshotBytes)
            guard FileProviderGenerationCodec.digest(data) == unit.digest else {
                throw FileProviderSynchronizationError.corrupt
            }
            let snapshot = try FileProviderMetadataSnapshot.decode(
                data, expectedDomain: manifest.domain,
                expectedOwnerScopeFingerprint: manifest.domain.ownerScopeFingerprint)
            guard snapshot.libraryID == unit.libraryID,
                snapshot.projectionGeneration == record.generation
            else { throw FileProviderSynchronizationError.corrupt }
            return snapshot
        }
    }

    /// Called only under the writer lease, after the host source has revalidated its session.
    func commit(
        domain: FileProviderDomainIdentity, evidence: FileProviderAuthorizationEvidence,
        generation: UInt64, snapshots: [FileProviderMetadataSnapshot], catalog: Set<String>
    ) throws {
        guard
            case .permittedForMetadataRead = FileProviderAuthorizationBoundary.evaluate(
                evidence: evidence, expectedDomain: domain,
                protectedDataAvailable: protectedDataAvailable(), capability: capability)
        else { throw FileProviderSynchronizationError.unauthorized }
        guard generation > 0, catalog.count <= 128,
            Set(snapshots.map(\.libraryID)) == catalog, snapshots.count == catalog.count,
            snapshots.allSatisfy({ $0.projectionGeneration == generation })
        else { throw FileProviderSynchronizationError.incomplete }
        let directory = try container.generationDirectory(domainID: domain.rawValue)
        let commitURL = directory.appendingPathComponent("commit.json")
        var previous: FileProviderGenerationManifest?
        var existingRecords: [FileProviderGenerationRecord] = []
        if FileManager.default.fileExists(atPath: commitURL.path) {
            // Revoked evidence must not be reused. A new authorization epoch may publish, but it
            // never inherits old anchors or data; generation/authorization rollback is rejected.
            let data = try readFile(
                commitURL, limit: FileProviderSharedContainer.maximumSnapshotBytes)
            let old = try JSONDecoder().decode(FileProviderGenerationManifest.self, from: data)
            guard old.schemaVersion == FileProviderGenerationManifest.format,
                old.domain == domain, let last = old.records.last,
                generation > last.generation,
                evidence.authorizationGeneration >= old.authorization.authorizationGeneration
            else { throw FileProviderSynchronizationError.rollback }
            existingRecords = old.records
            if evidence.authorizationGeneration == old.authorization.authorizationGeneration {
                guard old.authorization.state == .authorized else {
                    throw FileProviderSynchronizationError.unauthorized
                }
                previous = old
            }
        }
        // Validate every unit before any disk mutation. Encoding also enforces the 8 MiB bound.
        let encoded = try snapshots.sorted { $0.libraryID < $1.libraryID }.map { snapshot in
            try snapshot.encoded(
                expectedDomain: domain, expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint)
        }
        // Reclaim only orphan files; the current commit and every retained reader generation stay.
        try compact(directory: directory, retaining: existingRecords)
        var units: [FileProviderLibraryUnit] = []
        for (snapshot, data) in zip(snapshots.sorted { $0.libraryID < $1.libraryID }, encoded) {
            let filename = "unit-\(UUID().uuidString).json"
            try container.checkGenerationQuota(addingBytes: data.count)
            try write(data, to: directory.appendingPathComponent(filename))
            units.append(
                FileProviderLibraryUnit(
                    libraryID: snapshot.libraryID, filename: filename,
                    digest: FileProviderGenerationCodec.digest(data)))
            try fault("unitDurable")
        }
        // Coarse container notifications are bounded and deduplicated. Old parents are included
        // for moves and deletions, including notifications not acknowledged before a crash.
        let acknowledged =
            try previous.map { try signalsAcknowledged(domainID: domain.rawValue, manifest: $0) }
            ?? false
        var signals = Set(acknowledged ? [] : previous?.records.flatMap(\.pendingSignals) ?? [])
        signals.insert(FileProviderMetadataReader.root)
        signals.insert(FileProviderMetadataReader.workingSet)
        let oldSnapshots =
            try previous?.records.last.map { try self.snapshots($0, manifest: previous!) } ?? []
        let oldItems = Dictionary(
            uniqueKeysWithValues: oldSnapshots.flatMap(\.items).map { ($0.identity.rawValue, $0) })
        let newItems = Dictionary(
            uniqueKeysWithValues: snapshots.flatMap(\.items).map { ($0.identity.rawValue, $0) })
        for id in Set(oldItems.keys).union(newItems.keys) where oldItems[id] != newItems[id] {
            for item in [oldItems[id], newItems[id]].compactMap({ $0 }) {
                if let parent = item.parentIdentity { signals.insert(parent.rawValue) }
                if item.kind == .libraryRoot { signals.insert(item.identity.rawValue) }
            }
        }
        signals.remove(FileProviderItemIdentity.domainRoot(domain: domain).rawValue)
        let record = FileProviderGenerationRecord(
            generation: generation, libraries: units, pendingSignals: signals.sorted())
        let history = (previous?.records ?? []).map {
            FileProviderGenerationRecord(
                generation: $0.generation, libraries: $0.libraries, pendingSignals: [])
        }
        let records = Array(
            (history + [record]).suffix(
                FileProviderGenerationManifest.retainedGenerations))
        let manifest = FileProviderGenerationManifest(
            schemaVersion: FileProviderGenerationManifest.format, domain: domain,
            authorization: evidence, records: records)
        let data = try FileProviderGenerationCodec.encode(manifest)
        guard data.count <= FileProviderSharedContainer.maximumSnapshotBytes else {
            throw FileProviderSynchronizationError.incomplete
        }
        try container.checkGenerationQuota(addingBytes: data.count)
        try fault("beforeCommit")
        try write(data, to: commitURL)
        // A crash here leaves a fully usable commit. Compaction is always recoverable.
        try? compact(directory: directory, retaining: records)
    }

    func revoke(domainID: String, state: FileProviderAuthorizationState) throws {
        guard state != .authorized else { throw FileProviderSynchronizationError.unauthorized }
        try withLease(domainID: domainID, writing: true) {
            let manifest = try readManifest(domainID: domainID)
            let replacement = FileProviderGenerationManifest(
                schemaVersion: manifest.schemaVersion, domain: manifest.domain,
                authorization: FileProviderAuthorizationEvidence(
                    domain: manifest.domain, state: state,
                    authorizationGeneration: manifest.authorization.authorizationGeneration),
                records: manifest.records)
            let url = try container.url(for: ["generations", domainID, "commit.json"])
            try write(FileProviderGenerationCodec.encode(replacement), to: url)
        }
    }

    func readFile(_ url: URL, limit: Int) throws -> Data {
        let fd = open(url.path, O_RDONLY | O_NOFOLLOW)
        guard fd >= 0 else { throw FileProviderSynchronizationError.unavailable }
        let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? handle.close() }
        var info = stat()
        guard fstat(fd, &info) == 0, (info.st_mode & S_IFMT) == S_IFREG,
            info.st_size >= 0, info.st_size <= limit
        else { throw FileProviderSynchronizationError.corrupt }
        let data = try handle.read(upToCount: limit + 1) ?? Data()
        guard data.count == Int(info.st_size) else {
            throw FileProviderSynchronizationError.corrupt
        }
        return data
    }

    func write(_ data: Data, to destination: URL) throws {
        let staging = destination.deletingLastPathComponent().appendingPathComponent(
            ".stage-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: staging) }
        let fd = open(staging.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
        guard fd >= 0 else { throw FileProviderSynchronizationError.unavailable }
        let handle = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? handle.close() }
        try FileManager.default.setAttributes(
            [.protectionKey: FileProtectionType.complete], ofItemAtPath: staging.path)
        try handle.write(contentsOf: data)
        try handle.synchronize()
        try fault("fileDurable")
        guard rename(staging.path, destination.path) == 0 else {
            throw FileProviderSynchronizationError.unavailable
        }
        let directoryFD = open(destination.deletingLastPathComponent().path, O_RDONLY | O_NOFOLLOW)
        guard directoryFD >= 0 else { throw FileProviderSynchronizationError.unavailable }
        defer { close(directoryFD) }
        guard fsync(directoryFD) == 0 else { throw FileProviderSynchronizationError.unavailable }
    }

    private func compact(directory: URL, retaining records: [FileProviderGenerationRecord]) throws {
        let keep = Set(records.flatMap(\.libraries).map(\.filename))
        let files = try FileManager.default.contentsOfDirectory(
            at: directory, includingPropertiesForKeys: nil)
        guard files.count <= 8192 else {
            throw FileProviderSharedContainerError.storageQuotaExceeded
        }
        for file in files
        where file.lastPathComponent.hasPrefix("unit-")
            || file.lastPathComponent.hasPrefix(".stage-")
        {
            _ = try container.url(for: [
                "generations", directory.lastPathComponent, file.lastPathComponent,
            ])
            if !keep.contains(file.lastPathComponent) {
                try FileManager.default.removeItem(at: file)
            }
        }
    }
}
