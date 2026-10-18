import Foundation
import XCTest

@testable import Synveil

func inboundError(_ code: String, status: Int) throws -> HTTPTransportResponse {
    try queueHTTP(
        [
            "error": [
                "code": code, "message": "Private server diagnostics",
                "request_id": "sync-error-01", "retryable": false,
            ]
        ], status: status)
}

/// Real authenticated transport boundary with deterministic dispatch/response gates. Database
/// assertions in hooks examine file-backed production commits before allowing the response.
actor InboundWire: HTTPTransportProtocol {
    private var script: [HTTPTransportResponse] = []
    private var sent: [HTTPTransportRequest] = []
    private var failures: [Int: SynveilTransportError] = [:]
    private var gates: [Int: QueueGate] = [:]
    private var hook: (@Sendable (Int, HTTPTransportRequest) async throws -> Void)?
    func configure(_ responses: [HTTPTransportResponse]) { script = responses }
    func fail(at index: Int, _ error: SynveilTransportError) { failures[index] = error }
    func gate(at index: Int, _ gate: QueueGate) { gates[index] = gate }
    func inspect(_ hook: @escaping @Sendable (Int, HTTPTransportRequest) async throws -> Void) {
        self.hook = hook
    }
    func requests() -> [HTTPTransportRequest] { sent }
    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        let index = sent.count
        sent.append(request)
        let response = script.isEmpty ? nil : script.removeFirst()
        if let gate = gates[index] { await gate.arrive() }
        try await hook?(index, request)
        if let failure = failures[index] { throw failure }
        guard let response else { throw SynveilTransportError.offline }
        return response
    }
}

@MainActor
struct InboundFixture {
    let base: QueueFixture
    let wire: InboundWire
    let provider: AuthenticatedLibraryRequestProvider
    let queue: DurableMutationQueue
    let nodes: ProjectionNodes
    let projection: SQLiteNodeProjectionRepository
    let application: SyncFeedApplicationService
    let ack: SyncAckService
    let coordinator: InboundSyncCoordinator
    var scope: ClientMutationScope { base.scope }
    var database: MutationQueueSQLiteStore { base.database }

    func stage(from: UInt64 = 0, count: Int = 1, kind: SyncChangeKind = .nodeRenamed) async throws
        -> SyncFeedPage
    {
        let page = try await feedPage(
            scope: scope, object: inboundObject(scope, from: from, count: count, kind: kind),
            from: String(from))
        _ = try await database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        return page
    }
    func run(
        _ configuration: InboundSyncRunConfiguration = .foreground,
        progress: @MainActor (InboundSyncProgress) -> Void = { _ in }
    ) async -> InboundSyncRunResult {
        await coordinator.synchronize(
            scope: scope, configuration: configuration, progress: progress)
    }
    func record(from: String = "0") async throws -> InboundSyncPageRecord? {
        try await database.inboundPage(
            scope: scope, position: feedPosition(from: from),
            credentialId: queueUUID(92), bridge: QueueValidator())
    }
    func configure(
        pages: Int = 1, count: Int = 1, kind: SyncChangeKind = .nodeRenamed,
        high: UInt64? = nil, empty: Bool = true
    ) async throws {
        var script: [HTTPTransportResponse] = []
        for page in 0..<pages {
            let from = UInt64(page * count)
            script.append(
                try queueHTTP(
                    inboundObject(scope, from: from, count: count, kind: kind, high: high)))
            script.append(
                try queueHTTP(
                    queueCheckpointObject(scope: scope, sequence: String(from + UInt64(count)))))
        }
        if empty {
            script.append(
                try queueHTTP(feedObject(scope: scope, count: 0, from: UInt64(pages * count))))
        }
        await wire.configure(script)
    }
}

func inboundObject(
    _ scope: ClientMutationScope, from: UInt64 = 0, count: Int = 1,
    kind: SyncChangeKind = .nodeRenamed, high: UInt64? = nil
) -> [String: Any] {
    var object = feedObject(scope: scope, count: count, from: from, high: high)
    var data = object["data"] as! [String: Any]
    var changes: [[String: Any]] = []
    for index in 0..<count {
        let offset = Int(from) + index + 1
        let sequence = String(from + UInt64(index) + 1)
        let parent: Int? = kind == .nodePurged ? nil : 2
        changes.append(
            projectionEvent(
                kind, id: 10 + offset, event: 1000 + offset,
                sequence: sequence, parent: parent))
    }
    data["changes"] = changes
    if count > 0 { data["ack_token"] = "v1.original-page-\(from)" }
    object["data"] = data
    return object
}

@MainActor
func inboundFixture(
    _ test: XCTestCase, initializeBase: Bool = true,
    fault: QueueFaultInjector? = nil, base: QueueFixture? = nil,
    database: MutationQueueSQLiteStore? = nil
) async throws -> InboundFixture {
    let f: QueueFixture
    if let base {
        f = base
    } else {
        f = try await queueFixture(test, initializeBase: initializeBase, fault: fault)
    }
    let db = database ?? f.database
    let wire = InboundWire()
    let provider = AuthenticatedLibraryRequestProvider(
        controller: f.controller, store: f.credentials, transport: wire)
    let queue = DurableMutationQueue(store: db, provider: provider, bridge: QueueValidator())
    f.controller.installMutationSessionInvalidator { [weak queue] in queue?.invalidateSession() }
    _ = try await queue.capture(scope: f.scope)
    let nodes = ProjectionNodes()
    for index in 1...16 {
        let node = try await projectionNode(scope: f.scope, id: index + 10)
        nodes.values[node.id] = .loaded(node)
    }
    let parent = try await projectionNode(scope: f.scope, id: 2, parent: nil, kind: .directory)
    nodes.values[parent.id] = .loaded(parent)
    let projection = SQLiteNodeProjectionRepository(
        database: db, provider: provider, bridge: QueueValidator())
    let application = SyncFeedApplicationService(
        provider: provider, queue: queue, database: db, nodes: nodes, bridge: QueueValidator())
    let ack = SyncAckService(
        provider: provider,
        projection: CommittedSQLiteSyncProjectionStorage(database: db, bridge: QueueValidator()),
        bridge: QueueValidator())
    let coordinator = InboundSyncCoordinator(
        feed: SyncFeedService(
            provider: provider, queue: queue, store: db, bridge: QueueValidator()),
        application: application, ack: ack, projection: projection, queue: queue,
        database: db, provider: provider, bridge: QueueValidator())
    return InboundFixture(
        base: f, wire: wire, provider: provider, queue: queue, nodes: nodes,
        projection: projection, application: application, ack: ack, coordinator: coordinator)
}

@MainActor
func inboundLibrary(
    _ f: InboundFixture, status: LibraryStatus = .active,
    name: String = "Library with a long name 📚"
) async throws -> Library {
    Library(
        id: f.scope.libraryId, revision: try LibraryRevision(validating: "1"), name: name,
        rootNodeId: try await NodeId.validated(queueUUID(2), using: QueueValidator()),
        status: status,
        createdAt: Date(timeIntervalSince1970: 1_700_000_000),
        updatedAt: Date(timeIntervalSince1970: 1_700_000_001))
}

@MainActor
final class InboundProjectionReadGate: NodeProjectionRepositoryProtocol {
    let base: SQLiteNodeProjectionRepository
    let gate: QueueGate
    init(base: SQLiteNodeProjectionRepository, gate: QueueGate) {
        self.base = base
        self.gate = gate
    }
    func node(scope: ClientMutationScope, nodeId: NodeId) async -> CachedNodeResult {
        await base.node(scope: scope, nodeId: nodeId)
    }
    func children(scope: ClientMutationScope, parentId: NodeId, limit: Int) async
        -> CachedChildrenResult
    {
        await base.children(scope: scope, parentId: parentId, limit: limit)
    }
    func projectionState(scope: ClientMutationScope) async -> NodeProjectionStateResult {
        let value = await base.projectionState(scope: scope)
        await gate.arrive()
        return value
    }
}

/// Fault callbacks run on SQLite's actor; this synchronized cancellation handle carries no data.
final class InboundCancellationHandle: @unchecked Sendable {
    private let lock = NSLock()
    private var task: Task<InboundSyncRunResult, Never>?
    func install(_ task: Task<InboundSyncRunResult, Never>) {
        lock.lock()
        self.task = task
        lock.unlock()
    }
    func cancel() {
        lock.lock()
        let current = task
        lock.unlock()
        current?.cancel()
    }
}
