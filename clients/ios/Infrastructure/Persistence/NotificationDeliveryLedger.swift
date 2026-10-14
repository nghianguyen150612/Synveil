import Foundation

#if canImport(Darwin)
    import Darwin
#else
    import Glibc
#endif

/// Bounded presentation-only snapshot. Uses the existing private-file protection policy, with
/// fsync/rename/directory fsync and a cross-instance lock. It never writes the operation database.
actor NotificationDeliveryLedger: NotificationLedgerStorage {
    private struct Snapshot: Codable {
        var version = 1
        var events: [NotificationEvent] = []
    }
    enum Fault: Sendable { case beforeWrite, beforeRename, afterRename }
    private let url: URL
    private let fault: (@Sendable (Fault) throws -> Void)?
    init(url: URL, fault: (@Sendable (Fault) throws -> Void)? = nil) throws {
        guard url.isFileURL else { throw NotificationFailure.storageFailure }
        self.url = url
        self.fault = fault
        let lockURL = URL(fileURLWithPath: url.path + ".lock")
        try MutationQueueFilePolicy.prepare(lockURL)
        let descriptor = open(lockURL.path, O_RDWR | O_NOFOLLOW)
        guard descriptor >= 0 else { throw NotificationFailure.storageFailure }
        defer { close(descriptor) }
        guard flock(descriptor, LOCK_EX | LOCK_NB) == 0 else {
            throw NotificationFailure.storageFailure
        }
        defer { _ = flock(descriptor, LOCK_UN) }
        if !FileManager.default.fileExists(atPath: url.path) {
            try MutationQueueFilePolicy.prepare(url)
            try Data("{\"version\":1,\"events\":[]}".utf8).write(to: url)
        } else {
            try MutationQueueFilePolicy.protect(url)
        }
    }
    static func productionURL() throws -> URL {
        try MutationQueueSQLiteStore.productionURL().deletingLastPathComponent()
            .deletingLastPathComponent().appendingPathComponent("Notifications", isDirectory: true)
            .appendingPathComponent("delivery-v1.json")
    }
    func events() throws -> [NotificationEvent] {
        try locked { try read().events }
    }
    func insert(_ event: NotificationEvent) throws -> NotificationEvent {
        try locked {
            guard event.isValid, event.state == .eligible else {
                throw NotificationFailure.payloadRejected
            }
            var snapshot = try read()
            // Retired generations cannot be routed or published. Their dedup keys are never reused.
            snapshot.events.removeAll {
                $0.state == .invalidated && $0.generation != event.generation
            }
            if let existing = snapshot.events.first(where: {
                $0.generation == event.generation && $0.candidate == event.candidate
            }) {
                return existing
            }
            guard snapshot.events.count < NotificationLimits.maximumRecords else {
                throw NotificationFailure.capacity
            }
            if event.candidate.category == .backgroundSync,
                snapshot.events.contains(where: {
                    $0.generation == event.generation && $0.candidate.category == .backgroundSync
                        && event.recordedAt.timeIntervalSince($0.recordedAt)
                            < NotificationLimits.syncQuietPeriod
                })
            {
                throw NotificationFailure.capacity
            }
            snapshot.events.append(event)
            try write(snapshot)
            return event
        }
    }
    func transition(_ id: UUID, from: NotificationDeliveryState, to: NotificationDeliveryState)
        throws -> Bool
    {
        try locked {
            var snapshot = try read()
            guard let index = snapshot.events.firstIndex(where: { $0.id == id }),
                snapshot.events[index].state == from,
                (from == .claimed && to == .scheduled)
                    || ([.claimed, .scheduled].contains(from) && to == .handled)
                    || to == .invalidated
            else { return false }
            snapshot.events[index].state = to
            try write(snapshot)
            return true
        }
    }
    func claim(_ id: UUID, now: Date) throws -> Bool {
        try locked {
            var snapshot = try read()
            guard let index = snapshot.events.firstIndex(where: { $0.id == id }),
                snapshot.events[index].state == .eligible, now < snapshot.events[index].expiresAt
            else { return false }
            guard
                snapshot.events.filter({
                    [.claimed, .scheduled].contains($0.state) && now < $0.expiresAt
                }).count < NotificationLimits.maximumPending
            else { throw NotificationFailure.capacity }
            snapshot.events[index].state = .claimed
            try write(snapshot)
            return true
        }
    }
    func invalidate(generation: UUID) throws -> [String] {
        try locked {
            var snapshot = try read()
            let ids = snapshot.events.filter { $0.generation == generation }.map(
                \.requestIdentifier)
            for index in snapshot.events.indices
            where snapshot.events[index].generation == generation {
                snapshot.events[index].state = .invalidated
            }
            try write(snapshot)
            return ids
        }
    }
    private func read() throws -> Snapshot {
        let descriptor = open(url.path, O_RDONLY | O_NOFOLLOW)
        guard descriptor >= 0 else { throw NotificationFailure.storageFailure }
        defer { close(descriptor) }
        var info = stat()
        guard fstat(descriptor, &info) == 0, info.st_mode & S_IFMT == S_IFREG,
            info.st_nlink == 1, info.st_size >= 0,
            info.st_size <= NotificationLimits.maximumBytes
        else { throw NotificationFailure.storageFailure }
        guard info.st_size > 0 else { throw NotificationFailure.storageFailure }
        var bytes = Data(count: Int(info.st_size))
        try bytes.withUnsafeMutableBytes { buffer in
            var offset = 0
            while offset < buffer.count {
                #if canImport(Darwin)
                    let count = Darwin.read(
                        descriptor, buffer.baseAddress!.advanced(by: offset), buffer.count - offset)
                #else
                    let count = Glibc.read(
                        descriptor, buffer.baseAddress!.advanced(by: offset), buffer.count - offset)
                #endif
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { throw NotificationFailure.storageFailure }
                offset += count
            }
        }
        do {
            let root = try JSONSerialization.jsonObject(with: bytes)
            try LibraryWireValidation.keys(root, required: ["version", "events"])
            guard let object = root as? [String: Any],
                let events = object["events"] as? [[String: Any]]
            else {
                throw NotificationFailure.storageFailure
            }
            for event in events {
                try LibraryWireValidation.keys(
                    event,
                    required: [
                        "id", "candidate", "generation", "lifecycleRevision", "recordedAt",
                        "expiresAt", "state",
                    ])
                guard let candidate = event["candidate"] else {
                    throw NotificationFailure.storageFailure
                }
                try LibraryWireValidation.keys(
                    candidate,
                    required: [
                        "category", "evidence", "operation", "library", "scopeFingerprint",
                        "proofFingerprint",
                    ])
            }
            let snapshot = try JSONDecoder().decode(Snapshot.self, from: bytes)
            guard snapshot.version == 1, snapshot.events.count <= NotificationLimits.maximumRecords,
                snapshot.events.allSatisfy(\.isValid),
                Set(snapshot.events.map(\.id)).count == snapshot.events.count,
                Set(snapshot.events.map(\.deduplicationKey)).count == snapshot.events.count
            else { throw NotificationFailure.storageFailure }
            return snapshot
        } catch { throw NotificationFailure.storageFailure }
    }
    private func write(_ snapshot: Snapshot) throws {
        let bytes = try JSONEncoder().encode(snapshot)
        guard bytes.count <= NotificationLimits.maximumBytes else {
            throw NotificationFailure.capacity
        }
        try fault?(.beforeWrite)
        let temporary = url.appendingPathExtension("write")
        // An interrupted unrenamed file is presentation garbage, never operation evidence.
        _ = unlink(temporary.path)
        try MutationQueueFilePolicy.prepare(temporary)
        let descriptor = open(temporary.path, O_WRONLY | O_TRUNC | O_NOFOLLOW)
        guard descriptor >= 0 else { throw NotificationFailure.storageFailure }
        defer {
            close(descriptor)
            _ = unlink(temporary.path)
        }
        try bytes.withUnsafeBytes { buffer in
            var offset = 0
            while offset < buffer.count {
                #if canImport(Darwin)
                    let written = Darwin.write(
                        descriptor, buffer.baseAddress!.advanced(by: offset), buffer.count - offset)
                #else
                    let written = Glibc.write(
                        descriptor, buffer.baseAddress!.advanced(by: offset), buffer.count - offset)
                #endif
                if written < 0 && errno == EINTR { continue }
                guard written > 0 else { throw NotificationFailure.storageFailure }
                offset += written
            }
        }
        guard fsync(descriptor) == 0 else { throw NotificationFailure.storageFailure }
        try fault?(.beforeRename)
        guard rename(temporary.path, url.path) == 0 else {
            throw NotificationFailure.storageFailure
        }
        let directory = open(url.deletingLastPathComponent().path, O_RDONLY | O_NOFOLLOW)
        guard directory >= 0 else { throw NotificationFailure.storageFailure }
        defer { close(directory) }
        guard fsync(directory) == 0 else { throw NotificationFailure.storageFailure }
        try fault?(.afterRename)
    }
    private func locked<T>(_ operation: () throws -> T) throws -> T {
        let descriptor = open(url.path + ".lock", O_RDWR | O_CREAT | O_NOFOLLOW, mode_t(0o600))
        guard descriptor >= 0 else { throw NotificationFailure.storageFailure }
        defer { close(descriptor) }
        guard flock(descriptor, LOCK_EX | LOCK_NB) == 0 else {
            throw NotificationFailure.storageFailure
        }
        defer { _ = flock(descriptor, LOCK_UN) }
        return try operation()
    }
}
