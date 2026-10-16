import Foundation

struct FileProviderItemPage {
    let items: [FileProviderProjectedItem]
    let next: Data?
}

struct FileProviderChangePage {
    let updated: [FileProviderProjectedItem]
    let deleted: [String]
    let anchor: Data
    let moreComing: Bool
}

/// Pure deterministic queries over one leased durable commit. Page/anchor cursors never contain
/// item arrays. Retained complete generations are the bounded journal; absence is deletion only
/// when comparing two accepted complete generations, never after a failed/incomplete refresh.
struct FileProviderMetadataReader {
    static let root = "root"
    static let workingSet = "workingSet"
    static let pageSize = 100
    let store: FileProviderGenerationStore
    let domainID: String

    func withView<T>(_ body: (FileProviderMetadataView) throws -> T) throws -> T {
        try store.withLease(domainID: domainID, writing: false) {
            let manifest = try store.readManifest(domainID: domainID)
            return try body(FileProviderMetadataView(store: store, manifest: manifest))
        }
    }
}

struct FileProviderMetadataView {
    let store: FileProviderGenerationStore
    let manifest: FileProviderGenerationManifest
    var latest: FileProviderGenerationRecord { manifest.records.last! }

    func lookup(_ identifier: String) throws -> FileProviderProjectedItem {
        let all = try liveItems(latest)
        guard let item = all.first(where: { $0.identity.rawValue == identifier }) else {
            throw FileProviderSynchronizationError.noSuchItem
        }
        return item
    }

    func validateContainer(_ container: String) throws {
        if [FileProviderMetadataReader.root, FileProviderMetadataReader.workingSet].contains(
            container)
        {
            return
        }
        guard try lookup(container).kind != .file else {
            throw FileProviderSynchronizationError.noSuchItem
        }
    }

    func page(container: String, token: Data?, ordering: String = "page") throws
        -> FileProviderItemPage
    {
        try validateContainer(container)
        let cursor = try token.map { data in
            let decoded: FileProviderCursor
            do { decoded = try FileProviderCursor.decode(data) } catch {
                throw FileProviderSynchronizationError.pageExpired
            }
            guard ["page", "name", "date"].contains(decoded.purpose) else {
                throw FileProviderSynchronizationError.pageExpired
            }
            return try checked(data, purpose: decoded.purpose, container: container)
        }
        let order = cursor?.purpose ?? ordering
        guard ["page", "name", "date"].contains(order) else {
            throw FileProviderSynchronizationError.pageExpired
        }
        let generation = cursor?.generation ?? latest.generation
        guard let record = manifest.records.first(where: { $0.generation == generation }),
            cursor == nil || cursor?.target == generation,
            Set(record.libraries.map(\.libraryID)).isSubset(
                of: Set(latest.libraries.map(\.libraryID)))
        else { throw FileProviderSynchronizationError.pageExpired }
        let items = try children(record, container: container).sorted { left, right in
            if order == "name", left.displayName != right.displayName {
                return left.displayName < right.displayName
            }
            if order == "date", left.contentModificationDate != right.contentModificationDate {
                return (left.contentModificationDate ?? .distantPast)
                    > (right.contentModificationDate ?? .distantPast)
            }
            return left.identity.rawValue < right.identity.rawValue
        }
        let offset = cursor?.offset ?? 0
        guard offset <= items.count, offset % FileProviderMetadataReader.pageSize == 0 else {
            throw FileProviderSynchronizationError.pageExpired
        }
        let end = min(offset + FileProviderMetadataReader.pageSize, items.count)
        let next =
            end < items.count
            ? try makeCursor(order, container, generation, generation, end).encoded() : nil
        return FileProviderItemPage(items: Array(items[offset..<end]), next: next)
    }

    func currentAnchor(container: String) throws -> Data {
        try validateContainer(container)
        // Reading/validating the complete graph is required before announcing its durable point.
        _ = try liveItems(latest)
        return try makeCursor("anchor", container, latest.generation, latest.generation, 0)
            .encoded()
    }

    func changes(container: String, anchor: Data) throws -> FileProviderChangePage {
        let cursor = try checked(anchor, purpose: "anchor", container: container)
        guard let base = manifest.records.first(where: { $0.generation == cursor.generation })
        else {
            throw FileProviderSynchronizationError.anchorExpired
        }
        let targetGeneration = cursor.offset == 0 ? latest.generation : cursor.target
        guard targetGeneration >= base.generation,
            let target = manifest.records.first(where: { $0.generation == targetGeneration }),
            Set(target.libraries.map(\.libraryID)).isSubset(
                of: Set(latest.libraries.map(\.libraryID)))
        else { throw FileProviderSynchronizationError.anchorExpired }
        // A removed container can replay deletions of its previously enumerated children.
        let oldItems = try children(base, container: container, allowMissing: true)
        let newItems = try children(target, container: container, allowMissing: true)
        if ![FileProviderMetadataReader.root, FileProviderMetadataReader.workingSet].contains(
            container)
        {
            let containers = try liveItems(base) + liveItems(target)
            guard
                containers.contains(where: { $0.identity.rawValue == container && $0.kind != .file }
                )
            else {
                throw FileProviderSynchronizationError.noSuchItem
            }
        }
        let old = Dictionary(uniqueKeysWithValues: oldItems.map { ($0.identity.rawValue, $0) })
        let new = Dictionary(uniqueKeysWithValues: newItems.map { ($0.identity.rawValue, $0) })
        let globallyLive = Dictionary(
            uniqueKeysWithValues: try liveItems(target).map { ($0.identity.rawValue, $0) })
        let ids = Set(old.keys).union(new.keys).filter { old[$0] != new[$0] }.sorted()
        guard cursor.offset <= ids.count,
            cursor.offset % FileProviderMetadataReader.pageSize == 0
        else { throw FileProviderSynchronizationError.anchorExpired }
        let end = min(cursor.offset + FileProviderMetadataReader.pageSize, ids.count)
        let batch = ids[cursor.offset..<end]
        let more = end < ids.count
        let next = makeCursor(
            "anchor", container, more ? base.generation : target.generation,
            target.generation, more ? end : 0)
        return FileProviderChangePage(
            updated: batch.compactMap { globallyLive[$0] },
            deleted: batch.filter { globallyLive[$0] == nil },
            anchor: try next.encoded(), moreComing: more || target.generation < latest.generation)
    }

    private func liveItems(_ record: FileProviderGenerationRecord) throws
        -> [FileProviderProjectedItem]
    {
        try store.items(record, manifest: manifest).filter { !$0.isTombstone }
            .sorted { $0.identity.rawValue < $1.identity.rawValue }
    }

    private func children(
        _ record: FileProviderGenerationRecord, container: String, allowMissing: Bool = false
    ) throws -> [FileProviderProjectedItem] {
        let items = try liveItems(record)
        if container == FileProviderMetadataReader.workingSet { return items }
        if container == FileProviderMetadataReader.root {
            return items.filter { $0.kind == .libraryRoot }
        }
        // Validate the entire ID, Library and graph instead of accepting a matching parent string.
        guard let parent = items.first(where: { $0.identity.rawValue == container }),
            parent.kind != .file
        else {
            if allowMissing { return [] }
            throw FileProviderSynchronizationError.noSuchItem
        }
        return items.filter { $0.parentIdentity?.rawValue == container }
    }

    private func checked(_ data: Data, purpose: String, container: String) throws
        -> FileProviderCursor
    {
        let invalid: FileProviderSynchronizationError =
            purpose == "anchor" ? .anchorExpired : .pageExpired
        let cursor: FileProviderCursor
        do { cursor = try FileProviderCursor.decode(data) } catch { throw invalid }
        guard cursor.purpose == purpose,
            cursor.domain == FileProviderCursor.scope(manifest.domain.rawValue),
            cursor.owner == FileProviderCursor.scope(manifest.domain.ownerScopeFingerprint),
            cursor.authorization == manifest.authorization.authorizationGeneration,
            cursor.container == FileProviderCursor.scope(container), cursor.generation > 0
        else { throw invalid }
        return cursor
    }

    private func makeCursor(
        _ purpose: String, _ container: String, _ generation: UInt64, _ target: UInt64,
        _ offset: Int
    ) -> FileProviderCursor {
        FileProviderCursor(
            version: 1, purpose: purpose,
            domain: FileProviderCursor.scope(manifest.domain.rawValue),
            owner: FileProviderCursor.scope(manifest.domain.ownerScopeFingerprint),
            authorization: manifest.authorization.authorizationGeneration,
            container: FileProviderCursor.scope(container), generation: generation, target: target,
            offset: offset)
    }
}
