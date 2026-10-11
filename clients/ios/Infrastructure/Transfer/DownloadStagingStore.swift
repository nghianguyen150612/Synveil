import CryptoKit
import Foundation

#if canImport(Darwin)
    import Darwin
#else
    import Glibc
#endif

enum DownloadFilesystemFault: Sendable {
    case create, write, synchronize, truncate, rename, afterRename, read
}

/// A single actor accounts for all private transfer files and reservations, including unknown
/// future upload files. Paths are never inputs to content operations; openat/O_NOFOLLOW and
/// fstat check actual descriptors. No memory buffer grows with the file length.
actor DownloadStagingStore {
    private let root: URL
    private var hashers: [DownloadOperationId: SHA256] = [:]
    private var reservations: [DownloadOperationId: UInt64] = [:]
    private let freeBytes: @Sendable (URL) throws -> UInt64
    private let fault: (@Sendable (DownloadFilesystemFault) throws -> Void)?

    init(
        root: URL,
        freeBytes: @escaping @Sendable (URL) throws -> UInt64 = { url in
            let attributes = try FileManager.default.attributesOfFileSystem(forPath: url.path)
            guard let size = attributes[.systemFreeSize] as? NSNumber else {
                throw DownloadFailure.insufficientStorage
            }
            return size.uint64Value
        }, fault: (@Sendable (DownloadFilesystemFault) throws -> Void)? = nil
    ) throws {
        // Foundation may add a directory slash to standardizedFileURL once the directory
        // exists. Compare paths so reopening preserves the same canonical-path policy.
        guard root.isFileURL, root.standardizedFileURL.path == root.path else {
            throw DownloadFailure.filesystemFailure
        }
        self.root = root
        self.freeBytes = freeBytes
        self.fault = fault
        try Self.rejectLinks(root)
        try FileManager.default.createDirectory(
            at: root, withIntermediateDirectories: true,
            attributes: MutationQueueFilePolicy.requestedAttributes(directory: true))
        try Self.protect(root, directory: true)
    }

    static func productionURL() throws -> URL {
        guard
            let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first
        else { throw DownloadFailure.filesystemFailure }
        return caches.resolvingSymlinksInPath().appendingPathComponent(
            "staging/downloads", isDirectory: true)
    }

    func reserve(_ reservation: DownloadStorageReservation, durable: [DownloadStorageReservation])
        throws
    {
        var proposed: [DownloadOperationId: UInt64] = [:]
        for value in durable { proposed[value.operationId] = value.bytes }
        for (key, value) in reservations { proposed[key] = value }
        proposed[reservation.operationId] = reservation.bytes
        let actual = try inventory()
        var used: UInt64 = 0
        var unwritten: UInt64 = 0
        for (id, value) in proposed {
            let stored = actual[id.rawValue] ?? 0
            used = try add(used, max(stored, value))
            unwritten = try add(unwritten, value > stored ? value - stored : 0)
        }
        for (name, bytes) in actual where !proposed.keys.contains(where: { $0.rawValue == name }) {
            used = try add(used, bytes)
        }
        guard used <= DownloadPolicy.quota else { throw DownloadFailure.stagingQuotaExceeded }
        guard try freeBytes(root) >= add(unwritten, DownloadPolicy.freeReserve) else {
            throw DownloadFailure.insufficientStorage
        }
        reservations = proposed
    }

    func create(_ id: DownloadOperationId) throws {
        try fault?(.create)
        let directory = try openDirectory()
        defer { close(directory) }
        let descriptor = openat(
            directory, name(id, verified: false), O_CREAT | O_EXCL | O_WRONLY | O_NOFOLLOW, 0o600)
        guard descriptor >= 0 else { throw DownloadFailure.filesystemFailure }
        defer { close(descriptor) }
        try Self.protect(url(id, verified: false), directory: false)
        try synchronize(descriptor)
        try synchronize(directory)
        hashers[id] = SHA256()
    }

    /// Truncate only bytes not acknowledged by SQLite, then prove the entire committed prefix.
    func recover(_ record: DownloadRecord) throws -> DownloadRecoveryDecision {
        hashers[record.id] = nil
        if exists(record.id, verified: true) {
            guard record.checkpoint.offset == record.identity.expected.byteLength,
                record.state == .verifying || record.state == .verified || record.state.canResume
            else { throw DownloadFailure.integrityFailure }
            try verify(record, published: true)
            return .verifyPublished
        }
        let descriptor = try openFile(record.id, verified: false, writable: true)
        defer { close(descriptor) }
        let size = try length(descriptor)
        guard size >= record.checkpoint.offset else { throw DownloadFailure.integrityFailure }
        if size > record.checkpoint.offset {
            try fault?(.truncate)
            guard ftruncate(descriptor, off_t(record.checkpoint.offset)) == 0 else {
                throw DownloadFailure.filesystemFailure
            }
            try synchronize(descriptor)
        }
        let hasher = try hash(descriptor, count: record.checkpoint.offset)
        guard digest(hasher) == record.checkpoint.prefixSHA256 else {
            throw DownloadFailure.integrityFailure
        }
        hashers[record.id] = hasher
        return .resume(record.checkpoint)
    }

    func append(_ data: Data, record: DownloadRecord) throws -> DownloadCheckpoint {
        guard record.checkpoint.offset <= record.identity.expected.byteLength,
            !data.isEmpty, data.count <= DownloadPolicy.bufferSize,
            var hasher = hashers[record.id],
            UInt64(data.count) <= record.identity.expected.byteLength - record.checkpoint.offset
        else { throw DownloadFailure.contentLengthMismatch }
        guard try freeBytes(root) >= add(UInt64(data.count), DownloadPolicy.freeReserve) else {
            throw DownloadFailure.insufficientStorage
        }
        let descriptor = try openFile(record.id, verified: false, writable: true)
        defer { close(descriptor) }
        guard try length(descriptor) == record.checkpoint.offset,
            lseek(descriptor, off_t(record.checkpoint.offset), SEEK_SET) >= 0
        else { throw DownloadFailure.integrityFailure }
        try fault?(.write)
        try data.withUnsafeBytes { bytes in
            var offset = 0
            while offset < bytes.count {
                let written = write(
                    descriptor, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if written < 0, errno == EINTR { continue }
                guard written > 0 else { throw DownloadFailure.filesystemFailure }
                offset += written
            }
        }
        try synchronize(descriptor)
        hasher.update(data: data)
        hashers[record.id] = hasher
        return DownloadCheckpoint(
            offset: record.checkpoint.offset + UInt64(data.count), prefixSHA256: digest(hasher))
    }

    func verify(_ record: DownloadRecord, published: Bool = false) throws {
        let descriptor = try openFile(record.id, verified: published, writable: false)
        defer { close(descriptor) }
        guard try length(descriptor) == record.identity.expected.byteLength else {
            throw DownloadFailure.contentLengthMismatch
        }
        guard
            try digest(hash(descriptor, count: record.identity.expected.byteLength))
                == record.identity.expected.sha256
        else { throw DownloadFailure.sha256Mismatch }
        try synchronize(descriptor)
    }

    func publish(_ record: DownloadRecord) throws {
        // No writer ownership remains when the final full-file verification begins.
        hashers[record.id] = nil
        try verify(record)
        let directory = try openDirectory()
        defer { close(directory) }
        guard !exists(record.id, verified: true) else { throw DownloadFailure.integrityFailure }
        try fault?(.rename)
        guard
            renameat(
                directory, name(record.id, verified: false), directory,
                name(record.id, verified: true)) == 0
        else { throw DownloadFailure.filesystemFailure }
        try synchronize(directory)
        try fault?(.afterRename)
    }

    func readVerified(_ record: DownloadRecord, offset: UInt64, count: Int) throws -> Data {
        guard count >= 0, count <= DownloadPolicy.bufferSize,
            offset <= record.identity.expected.byteLength,
            UInt64(count) <= record.identity.expected.byteLength - offset
        else { throw DownloadFailure.invalidRange }
        try verify(record, published: true)
        let descriptor = try openFile(record.id, verified: true, writable: false)
        defer { close(descriptor) }
        guard lseek(descriptor, off_t(offset), SEEK_SET) >= 0 else {
            throw DownloadFailure.filesystemFailure
        }
        let handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: false)
        let data = try handle.read(upToCount: count) ?? Data()
        guard data.count == count else { throw DownloadFailure.integrityFailure }
        return data
    }

    /// Cleanup is best effort; durable authorization is revoked even when physical deletion fails.
    func discard(_ ids: [DownloadOperationId]) throws {
        let directory = try openDirectory()
        defer { close(directory) }
        for id in ids {
            hashers[id] = nil
            for verified in [false, true] {
                if unlinkat(directory, name(id, verified: verified), 0) != 0, errno != ENOENT {
                    throw DownloadFailure.filesystemFailure
                }
            }
            reservations[id] = nil
        }
        try synchronize(directory)
    }

    private func inventory() throws -> [String: UInt64] {
        let directory = try openDirectory()
        defer { close(directory) }
        var result: [String: UInt64] = [:]
        for file in try FileManager.default.contentsOfDirectory(atPath: root.path) {
            let descriptor = openat(directory, file, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
            guard descriptor >= 0 else { throw DownloadFailure.filesystemFailure }
            defer { close(descriptor) }
            let bytes = try length(descriptor)
            let key = (file as NSString).deletingPathExtension
            result[key] = try add(result[key] ?? 0, bytes)
        }
        return result
    }
    private func add(_ a: UInt64, _ b: UInt64) throws -> UInt64 {
        let sum = a.addingReportingOverflow(b)
        guard !sum.overflow else { throw DownloadFailure.stagingQuotaExceeded }
        return sum.partialValue
    }
    private func name(_ id: DownloadOperationId, verified: Bool) -> String {
        id.rawValue + (verified ? ".verified" : ".part")
    }
    private func url(_ id: DownloadOperationId, verified: Bool) -> URL {
        root.appendingPathComponent(name(id, verified: verified))
    }
    private func exists(_ id: DownloadOperationId, verified: Bool) -> Bool {
        FileManager.default.fileExists(atPath: url(id, verified: verified).path)
    }
    private func openDirectory() throws -> Int32 {
        try Self.rejectLinks(root)
        let descriptor = open(root.path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
        guard descriptor >= 0 else { throw DownloadFailure.filesystemFailure }
        return descriptor
    }
    private func openFile(_ id: DownloadOperationId, verified: Bool, writable: Bool) throws -> Int32
    {
        let directory = try openDirectory()
        defer { close(directory) }
        let descriptor = openat(
            directory, name(id, verified: verified),
            (writable ? O_RDWR : O_RDONLY) | O_NOFOLLOW | O_NONBLOCK)
        guard descriptor >= 0 else {
            throw errno == ENOENT
                ? DownloadFailure.stagingMissing : DownloadFailure.filesystemFailure
        }
        do { _ = try length(descriptor) } catch {
            close(descriptor)
            throw error
        }
        return descriptor
    }
    private func length(_ descriptor: Int32) throws -> UInt64 {
        var info = stat()
        guard fstat(descriptor, &info) == 0, (info.st_mode & S_IFMT) == S_IFREG,
            info.st_nlink == 1, info.st_size >= 0
        else { throw DownloadFailure.filesystemFailure }
        return UInt64(info.st_size)
    }
    private func synchronize(_ descriptor: Int32) throws {
        try fault?(.synchronize)
        guard fsync(descriptor) == 0 else { throw DownloadFailure.filesystemFailure }
    }
    private func hash(_ descriptor: Int32, count: UInt64) throws -> SHA256 {
        try fault?(.read)
        guard lseek(descriptor, 0, SEEK_SET) >= 0 else { throw DownloadFailure.filesystemFailure }
        let handle = FileHandle(fileDescriptor: descriptor, closeOnDealloc: false)
        var hasher = SHA256()
        var remaining = count
        while remaining > 0 {
            try Task.checkCancellation()
            let buffer =
                try handle.read(upToCount: Int(min(remaining, UInt64(DownloadPolicy.bufferSize))))
                ?? Data()
            guard !buffer.isEmpty else { throw DownloadFailure.integrityFailure }
            hasher.update(data: buffer)
            remaining -= UInt64(buffer.count)
        }
        return hasher
    }
    private func digest(_ hasher: SHA256) -> String {
        "sha256:" + hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }
    private static func rejectLinks(_ url: URL) throws {
        var candidate = url
        while candidate.path != "/" {
            if let attributes = try? FileManager.default.attributesOfItem(atPath: candidate.path),
                attributes[.type] as? FileAttributeType == .typeSymbolicLink
            {
                throw DownloadFailure.filesystemFailure
            }
            candidate.deleteLastPathComponent()
        }
    }
    private static func protect(_ url: URL, directory: Bool) throws {
        try rejectLinks(url)
        try FileManager.default.setAttributes(
            MutationQueueFilePolicy.requestedAttributes(directory: directory),
            ofItemAtPath: url.path)
        #if canImport(Darwin)
            var excluded = URLResourceValues()
            excluded.isExcludedFromBackup = true
            var file = url
            try file.setResourceValues(excluded)
        #endif
        #if os(iOS) && !targetEnvironment(simulator)
            let actual = try FileManager.default.attributesOfItem(atPath: url.path)
            guard
                MutationQueueFilePolicy.protectionName(actual)
                    == FileProtectionType.completeUntilFirstUserAuthentication.rawValue
            else { throw DownloadFailure.filesystemFailure }
        #endif
    }
}
