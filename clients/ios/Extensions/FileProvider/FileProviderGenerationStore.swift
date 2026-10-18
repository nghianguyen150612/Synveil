import Darwin
import Foundation

/// One host writer, immutable Library units and a single atomic commit manifest. Readers hold a
/// short nonblocking shared lease through delivery; revocation takes the exclusive lease. No
/// callback can deliver old evidence after a successful revocation, even if it loaded it earlier.
struct FileProviderGenerationStore {
    static let maximumUnits = 2048
    static let maximumGenerationBytes = 12 * 1024 * 1024
    static let maximumGenerationRecords = 50_000
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
        let pending = try container.url(for: ["generations", domainID, "unsettled"])
        guard !FileManager.default.fileExists(atPath: pending.path) else {
            throw FileProviderSynchronizationError.unavailable
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
            manifest.records.allSatisfy({
                $0.generation > 0 && $0.libraries.count <= Self.maximumUnits
            }),
            zip(manifest.records, manifest.records.dropFirst()).allSatisfy({
                $0.generation < $1.generation
            })
        else { throw FileProviderSynchronizationError.corrupt }
        return manifest
    }

    func items(
        _ record: FileProviderGenerationRecord, manifest: FileProviderGenerationManifest
    ) throws -> [FileProviderProjectedItem] {
        var libraries: Set<String> = []
        var containers: [FileProviderContainerSnapshot] = []
        var legacy: [FileProviderProjectedItem] = []
        var bytes = 0
        var records = 0
        for unit in record.libraries {
            guard unit.filename.hasPrefix("unit-"), unit.filename.hasSuffix(".json"),
                FileProviderSharedContainer.isSafePathComponent(unit.filename)
            else { throw FileProviderSynchronizationError.corrupt }
            let url = try container.url(for: [
                "generations", manifest.domain.rawValue, unit.filename,
            ])
            let data = try readFile(url, limit: FileProviderSharedContainer.maximumSnapshotBytes)
            bytes += data.count
            guard bytes <= Self.maximumGenerationBytes,
                FileProviderGenerationCodec.digest(data) == unit.digest
            else { throw FileProviderSynchronizationError.corrupt }
            if let containerID = unit.containerIdentity {
                let value = try FileProviderContainerSnapshot.decode(
                    data, domain: manifest.domain, generation: record.generation)
                guard value.libraryID == unit.libraryID, value.containerIdentity == containerID
                else {
                    throw FileProviderSynchronizationError.corrupt
                }
                records += value.ancestors.count + value.children.count
                containers.append(value)
            } else {
                let snapshot = try FileProviderMetadataSnapshot.decode(
                    data, expectedDomain: manifest.domain,
                    expectedOwnerScopeFingerprint: manifest.domain.ownerScopeFingerprint)
                guard libraries.insert(unit.libraryID).inserted,
                    snapshot.libraryID == unit.libraryID,
                    snapshot.projectionGeneration == record.generation
                else { throw FileProviderSynchronizationError.corrupt }
                records += snapshot.items.count
                legacy += snapshot.items
            }
            guard records <= Self.maximumGenerationRecords else {
                throw FileProviderSynchronizationError.incomplete
            }
        }
        guard libraries.isDisjoint(with: Set(containers.map(\.libraryID))),
            libraries.union(containers.map(\.libraryID)).count <= 128
        else { throw FileProviderSynchronizationError.corrupt }
        return try legacy
            + FileProviderContainerSnapshot.assemble(
                containers, domain: manifest.domain, generation: record.generation)
    }

    /// Called only under the writer lease, after the host source has revalidated its session.
    func commit(
        domain: FileProviderDomainIdentity, evidence: FileProviderAuthorizationEvidence,
        generation: UInt64, snapshots: [FileProviderMetadataSnapshot], catalog: Set<String>,
        containerSnapshots: [FileProviderContainerSnapshot] = []
    ) throws {
        guard
            case .permittedForMetadataRead = FileProviderAuthorizationBoundary.evaluate(
                evidence: evidence, expectedDomain: domain,
                protectedDataAvailable: protectedDataAvailable(), capability: capability)
        else { throw FileProviderSynchronizationError.unauthorized }
        guard generation > 0, catalog.count <= 128,
            Set(snapshots.map(\.libraryID)).union(containerSnapshots.map(\.libraryID)) == catalog,
            Set(snapshots.map(\.libraryID)).count == snapshots.count,
            Set(snapshots.map(\.libraryID)).isDisjoint(
                with: Set(containerSnapshots.map(\.libraryID))),
            snapshots.count + containerSnapshots.count <= Self.maximumUnits,
            snapshots.allSatisfy({ $0.projectionGeneration == generation }),
            snapshots.reduce(0, { $0 + $1.items.count })
                + containerSnapshots.reduce(0, { $0 + $1.ancestors.count + $1.children.count })
                <= Self.maximumGenerationRecords
        else { throw FileProviderSynchronizationError.incomplete }
        let directory = try container.generationDirectory(domainID: domain.rawValue)
        let commitURL = directory.appendingPathComponent("commit.json")
        try recoverUnsettled(directory: directory)
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
        let partitioned = try FileProviderContainerSnapshot.assemble(
            containerSnapshots, domain: domain, generation: generation)
        let newItems = snapshots.flatMap(\.items) + partitioned
        var encoded: [(String, String?, Data)] = []
        var byteCount = 0
        for snapshot in snapshots.sorted(by: { $0.libraryID < $1.libraryID }) {
            let data = try snapshot.encoded(
                expectedDomain: domain, expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint)
            byteCount += data.count
            guard byteCount <= Self.maximumGenerationBytes else {
                throw FileProviderMetadataContractError.snapshotTooLarge
            }
            encoded.append((snapshot.libraryID, nil, data))
        }
        for unit in containerSnapshots.sorted(by: { $0.containerIdentity < $1.containerIdentity }) {
            let data = try unit.encoded(domain: domain, generation: generation)
            byteCount += data.count
            guard byteCount <= Self.maximumGenerationBytes else {
                throw FileProviderMetadataContractError.snapshotTooLarge
            }
            encoded.append((unit.libraryID, unit.containerIdentity, data))
        }
        try compact(directory: directory, retaining: existingRecords)
        var units: [FileProviderLibraryUnit] = []
        for (libraryID, containerID, data) in encoded {
            let filename = "unit-\(UUID().uuidString).json"
            try container.checkGenerationQuota(addingBytes: data.count)
            try write(data, to: directory.appendingPathComponent(filename))
            units.append(
                FileProviderLibraryUnit(
                    libraryID: libraryID, filename: filename,
                    digest: FileProviderGenerationCodec.digest(data), containerIdentity: containerID
                ))
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
        let oldItems =
            try previous?.records.last.map { try self.items($0, manifest: previous!) } ?? []
        let oldByID = Dictionary(uniqueKeysWithValues: oldItems.map { ($0.identity.rawValue, $0) })
        let newByID = Dictionary(uniqueKeysWithValues: newItems.map { ($0.identity.rawValue, $0) })
        for id in Set(oldByID.keys).union(newByID.keys) where oldByID[id] != newByID[id] {
            for item in [oldByID[id], newByID[id]].compactMap({ $0 }) {
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
        try commitManifest(data, directory: directory)
        // A crash here leaves a fully usable commit. Compaction is always recoverable.
        try? compact(directory: directory, retaining: records)
    }

    func revoke(domainID: String, state: FileProviderAuthorizationState) throws {
        guard state != .authorized else { throw FileProviderSynchronizationError.unauthorized }
        try withLease(domainID: domainID, writing: true) {
            let directory = try container.generationDirectory(domainID: domainID)
            try recoverUnsettled(directory: directory)
            let manifest = try readManifest(domainID: domainID)
            let replacement = FileProviderGenerationManifest(
                schemaVersion: manifest.schemaVersion, domain: manifest.domain,
                authorization: FileProviderAuthorizationEvidence(
                    domain: manifest.domain, state: state,
                    authorizationGeneration: manifest.authorization.authorizationGeneration),
                records: manifest.records)
            try commitManifest(
                FileProviderGenerationCodec.encode(replacement), directory: directory)
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
        try fault("afterRename")
        let directoryFD = open(destination.deletingLastPathComponent().path, O_RDONLY | O_NOFOLLOW)
        guard directoryFD >= 0 else { throw FileProviderSynchronizationError.unavailable }
        defer { close(directoryFD) }
        guard fsync(directoryFD) == 0 else { throw FileProviderSynchronizationError.unavailable }
    }

    /// A durable unsettled marker fences visibility across rename/directory-fsync uncertainty.
    /// The rollback file is durable before the marker. Recovery never guesses whether rename
    /// survived: it restores the last acknowledged manifest before allowing another publication.
    private func commitManifest(_ data: Data, directory: URL) throws {
        let commit = directory.appendingPathComponent("commit.json")
        let rollback = directory.appendingPathComponent("rollback.json")
        let pending = directory.appendingPathComponent("unsettled")
        let prior =
            FileManager.default.fileExists(atPath: commit.path)
            ? try readFile(commit, limit: FileProviderSharedContainer.maximumSnapshotBytes) : Data()
        try container.checkGenerationQuota(addingBytes: prior.count + data.count + 1)
        try write(prior, to: rollback)
        // Until this write is durable commit.json is untouched, so a marker failure is safe.
        try write(Data([1]), to: pending)
        do {
            try fault("manifestWillReplace")
            try write(data, to: commit)
            try fault("manifestDurable")
            try FileManager.default.removeItem(at: pending)
            try synchronizeDirectory(directory)
        } catch {
            // If rollback cannot be made durable (disk full, I/O failure), leave the marker and
            // deny all reads. Never announce an anchor from an uncertain commit.
            var recovery = self
            recovery.fault = { _ in }
            try? recovery.recoverUnsettled(directory: directory)
            throw error
        }
    }

    private func recoverUnsettled(directory: URL) throws {
        let pending = directory.appendingPathComponent("unsettled")
        _ = try container.url(for: ["generations", directory.lastPathComponent, "unsettled"])
        guard FileManager.default.fileExists(atPath: pending.path) else { return }
        let rollback = try container.url(for: [
            "generations", directory.lastPathComponent, "rollback.json",
        ])
        let data = try readFile(rollback, limit: FileProviderSharedContainer.maximumSnapshotBytes)
        let commit = directory.appendingPathComponent("commit.json")
        if data.isEmpty {
            if FileManager.default.fileExists(atPath: commit.path) {
                try FileManager.default.removeItem(at: commit)
            }
            try synchronizeDirectory(directory)
        } else {
            try write(data, to: commit)
        }
        try FileManager.default.removeItem(at: pending)
        try synchronizeDirectory(directory)
    }

    private func synchronizeDirectory(_ directory: URL) throws {
        let fd = open(directory.path, O_RDONLY | O_NOFOLLOW)
        guard fd >= 0 else { throw FileProviderSynchronizationError.unavailable }
        defer { close(fd) }
        guard fsync(fd) == 0 else { throw FileProviderSynchronizationError.unavailable }
    }

    private func compact(directory: URL, retaining records: [FileProviderGenerationRecord]) throws {
        let keep = Set(records.flatMap(\.libraries).map(\.filename))
        var failed = false
        guard
            let enumerator = FileManager.default.enumerator(
                at: directory, includingPropertiesForKeys: nil,
                options: [.skipsSubdirectoryDescendants],
                errorHandler: { _, _ in
                    failed = true
                    return false
                })
        else {
            throw FileProviderSynchronizationError.unavailable
        }
        var files: [URL] = []
        for case let file as URL in enumerator {
            guard files.count < 8192 else {
                throw FileProviderSharedContainerError.storageQuotaExceeded
            }
            files.append(file)
        }
        guard !failed else { throw FileProviderSynchronizationError.unavailable }
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
