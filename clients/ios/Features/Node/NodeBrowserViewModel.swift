import Foundation
import Observation

/// Non-secret Library context carried by navigation destinations.
struct NodeBrowserLibraryContext: Equatable, Hashable, Sendable {
    let id: LibraryId
    let name: String
    let rootNodeId: NodeId

    init(_ library: Library) {
        id = library.id
        name = library.name
        rootNodeId = library.rootNodeId
    }
}

/// A directory destination is identified by canonical IDs and its typed parent scope.
struct NodeBrowserRoute: Equatable, Hashable, Sendable {
    let library: NodeBrowserLibraryContext
    let parentScope: NodeParentScope
    let directoryTitle: String
    let ancestry: [NodeId]

    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.library == rhs.library && lhs.parentScope == rhs.parentScope
            && lhs.directoryTitle == rhs.directoryTitle && lhs.ancestry == rhs.ancestry
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(library)
        switch parentScope {
        case .libraryRoot(let rootNodeId):
            hasher.combine(0)
            hasher.combine(rootNodeId)
        case .directory(let nodeId):
            hasher.combine(1)
            hasher.combine(nodeId)
        }
        hasher.combine(directoryTitle)
        hasher.combine(ancestry)
    }

    static func root(for library: Library) -> Self {
        let context = NodeBrowserLibraryContext(library)
        return Self(
            library: context,
            parentScope: .libraryRoot(rootNodeId: library.rootNodeId),
            directoryTitle: library.name,
            ancestry: [library.rootNodeId]
        )
    }
}

/// Safe, immutable fields selected from a validated file Node for its read-only details page.
struct NodeFileDetailsRoute: Equatable, Hashable, Sendable {
    let library: NodeBrowserLibraryContext
    let parentScope: NodeParentScope
    let ancestry: [NodeId]
    let parentDirectoryTitle: String
    let nodeId: NodeId
    let name: String
    let revision: NodeRevision
    let createdAt: Date
    let updatedAt: Date

    init(
        node: Node,
        library: NodeBrowserLibraryContext,
        parentScope: NodeParentScope,
        ancestry: [NodeId],
        parentDirectoryTitle: String
    ) {
        self.library = library
        self.parentScope = parentScope
        self.ancestry = ancestry
        self.parentDirectoryTitle = parentDirectoryTitle
        nodeId = node.id
        name = node.name
        revision = node.revision
        createdAt = node.createdAt
        updatedAt = node.updatedAt
    }

    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.library == rhs.library && lhs.parentScope == rhs.parentScope
            && lhs.ancestry == rhs.ancestry
            && lhs.parentDirectoryTitle == rhs.parentDirectoryTitle
            && lhs.nodeId == rhs.nodeId && lhs.name == rhs.name
            && lhs.revision == rhs.revision && lhs.createdAt == rhs.createdAt
            && lhs.updatedAt == rhs.updatedAt
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(library)
        switch parentScope {
        case .libraryRoot(let rootNodeId):
            hasher.combine(0)
            hasher.combine(rootNodeId)
        case .directory(let nodeId):
            hasher.combine(1)
            hasher.combine(nodeId)
        }
        hasher.combine(ancestry)
        hasher.combine(parentDirectoryTitle)
        hasher.combine(nodeId)
        hasher.combine(name)
        hasher.combine(revision)
        hasher.combine(createdAt)
        hasher.combine(updatedAt)
    }
}

enum NodeBrowserViewState: Equatable {
    case idle
    case loading
    case loaded([Node])
    case empty
    case refreshing([Node])
    case refreshFailed([Node], NodeBrowserUIFailure)
    case refreshCancelled([Node])
    case failed(NodeBrowserUIFailure)
    case cancelled
    case invalidated

    var visibleNodes: [Node]? {
        switch self {
        case .loaded(let nodes), .refreshing(let nodes), .refreshFailed(let nodes, _),
            .refreshCancelled(let nodes):
            nodes
        case .empty:
            []
        case .idle, .loading, .failed, .cancelled, .invalidated:
            nil
        }
    }

    var isRequestInProgress: Bool {
        switch self {
        case .loading, .refreshing: true
        case .idle, .loaded, .empty, .refreshFailed, .refreshCancelled, .failed, .cancelled,
            .invalidated:
            false
        }
    }
}

enum NodeBrowserUIFailure: Equatable {
    case repositoryUnavailable
    case node(NodeFailure)

    var title: String {
        switch self {
        case .repositoryUnavailable: "Folder browser unavailable"
        case .node(let failure):
            switch failure {
            case .unauthenticated: "Session unavailable"
            case .credentialUnavailable, .invalidCredential: "Device authorization needs recovery"
            case .originMismatch: "Server session could not be verified"
            case .staleSession: "Device session changed"
            case .authenticationRejected: "Device authorization rejected"
            case .deviceRevoked: "Device access revoked"
            case .offline: "No network connection"
            case .dnsFailure: "Server address not found"
            case .timeout: "Server did not respond"
            case .tlsFailure: "Secure connection could not be verified"
            case .redirectRejected: "Unexpected server redirect"
            case .serverUnavailable: "Server temporarily unavailable"
            case .httpFailure: "Folder request failed"
            case .unexpectedContentType, .protocolFailure, .repeatedCursor:
                "Folder response could not be verified"
            case .resourceLimit: "Folder contains too many items"
            case .cancelled: "Folder request cancelled"
            }
        }
    }

    var message: String {
        switch self {
        case .repositoryUnavailable:
            "Secure folder access is not ready. Reopen Synveil before browsing this Library."
        case .node(let failure):
            switch failure {
            case .unauthenticated:
                "This session is no longer available. Return to sign in before browsing folders."
            case .credentialUnavailable, .invalidCredential:
                "Synveil could not verify this device's saved authorization. "
                    + "Use the existing session recovery flow."
            case .originMismatch:
                "This folder could not be matched to the configured server. "
                    + "Its contents are hidden for safety."
            case .staleSession:
                "The device session changed while loading. "
                    + "Previously displayed folder data was cleared."
            case .authenticationRejected:
                "The server rejected this device session. Synveil is opening the recovery flow."
            case .deviceRevoked:
                "This device's access was revoked. Synveil is opening the recovery flow."
            case .offline:
                "Check your internet connection, then retry this read-only folder request."
            case .dnsFailure:
                "Check the network and configured server address, then retry."
            case .timeout:
                "The server did not respond in time. Check your connection and retry."
            case .tlsFailure:
                "Synveil could not verify the server's secure connection. Contact the server owner."
            case .redirectRejected:
                "The server returned a redirect that Synveil could not safely follow. "
                    + "Contact the server owner."
            case .serverUnavailable:
                "The server is temporarily unavailable. Wait a moment, then retry."
            case .httpFailure(let statusCode):
                "The server could not complete this request (HTTP \(statusCode))."
            case .unexpectedContentType, .protocolFailure, .repeatedCursor:
                "The server response did not match the expected folder format. "
                    + "Contact the server owner."
            case .resourceLimit:
                "This folder exceeds the client safety limit. Contact the server owner."
            case .cancelled:
                "The folder request was cancelled. You can retry when ready."
            }
        }
    }

    var accessibilityDescription: String { "\(title). \(message)" }

    var canRetry: Bool {
        switch self {
        case .repositoryUnavailable: false
        case .node(let failure):
            switch failure {
            case .offline, .dnsFailure, .timeout, .serverUnavailable, .cancelled:
                true
            case .httpFailure(let statusCode): (500...599).contains(statusCode)
            case .unauthenticated, .credentialUnavailable, .invalidCredential, .originMismatch,
                .staleSession, .authenticationRejected, .deviceRevoked, .tlsFailure,
                .redirectRejected, .unexpectedContentType, .protocolFailure, .resourceLimit,
                .repeatedCursor:
                false
            }
        }
    }

    var mayRetainPreviouslyLoadedData: Bool {
        guard case .node(let failure) = self else { return false }
        return switch failure {
        case .offline, .dnsFailure, .timeout, .serverUnavailable:
            true
        case .httpFailure(let statusCode):
            (500...599).contains(statusCode)
        case .unauthenticated, .credentialUnavailable, .invalidCredential, .originMismatch,
            .staleSession, .authenticationRejected, .deviceRevoked, .tlsFailure,
            .redirectRejected, .unexpectedContentType, .protocolFailure, .resourceLimit,
            .repeatedCursor, .cancelled:
            false
        }
    }
}

/// Owns one directory request and retains data only for this directory and authenticated revision.
@Observable
@MainActor
final class NodeBrowserViewModel {
    private(set) var state: NodeBrowserViewState = .idle

    let library: NodeBrowserLibraryContext
    let parentScope: NodeParentScope
    let directoryTitle: String
    let ancestry: [NodeId]

    private let repository: (any NodeRepositoryProtocol)?
    private let sessionController: SessionController
    private let sessionRevision: UInt64
    @ObservationIgnored private var didStartInitialLoad = false
    @ObservationIgnored private var activeRequestIdentity: RequestIdentity?
    @ObservationIgnored private var activeRepositoryTask: Task<NodeRepositoryResult, Never>?
    @ObservationIgnored private var operationGeneration: UInt64 = 0
    @ObservationIgnored private var invalidated = false

    init(
        repository: (any NodeRepositoryProtocol)?,
        sessionController: SessionController,
        route: NodeBrowserRoute
    ) {
        self.repository = repository
        self.sessionController = sessionController
        library = route.library
        parentScope = route.parentScope
        directoryTitle = route.directoryTitle
        ancestry = route.ancestry
        sessionRevision = sessionController.lifecycleRevision
    }

    var visibleNodes: [Node]? {
        guard isCurrentSession else { return nil }
        return state.visibleNodes
    }

    var isRequestInProgress: Bool { state.isRequestInProgress }

    var canRefresh: Bool {
        guard isCurrentSession, activeRequestIdentity == nil else { return false }
        return switch state {
        case .failed(let failure), .refreshFailed(_, let failure): failure.canRetry
        case .loading, .refreshing, .invalidated: false
        case .idle, .loaded, .empty, .refreshCancelled, .cancelled: true
        }
    }

    func loadIfNeeded() async {
        guard !didStartInitialLoad, case .idle = state else { return }
        guard validateCurrentSession() else { return }
        didStartInitialLoad = true
        await requestDirectory(isRefresh: false)
    }

    func refresh() async {
        guard validateCurrentSession(), activeRequestIdentity == nil else { return }
        didStartInitialLoad = true
        await requestDirectory(isRefresh: true)
    }

    func sessionDidChange() {
        guard !isCurrentSession else { return }
        invalidate()
    }

    func invalidate() {
        guard !invalidated else { return }
        invalidated = true
        activeRequestIdentity = nil
        operationGeneration &+= 1
        activeRepositoryTask?.cancel()
        activeRepositoryTask = nil
        state = .invalidated
    }

    func route(into node: Node) -> NodeBrowserRoute? {
        guard node.kind == .directory, node.libraryId == library.id,
            node.parentId == parentScope.expectedParentId, node.state == .active,
            !ancestry.contains(node.id)
        else { return nil }
        return NodeBrowserRoute(
            library: library,
            parentScope: .directory(node.id),
            directoryTitle: node.name,
            ancestry: ancestry + [node.id]
        )
    }

    func details(for node: Node) -> NodeFileDetailsRoute? {
        guard node.kind == .file, node.libraryId == library.id,
            node.parentId == parentScope.expectedParentId, node.state == .active
        else { return nil }
        return NodeFileDetailsRoute(
            node: node,
            library: library,
            parentScope: parentScope,
            ancestry: ancestry,
            parentDirectoryTitle: directoryTitle
        )
    }

    private var isCurrentSession: Bool {
        !invalidated && sessionController.state == .authenticated
            && sessionController.lifecycleRevision == sessionRevision
    }

    @discardableResult
    private func validateCurrentSession() -> Bool {
        guard isCurrentSession else {
            invalidate()
            return false
        }
        return true
    }

    private func requestDirectory(isRefresh: Bool) async {
        guard activeRequestIdentity == nil, validateCurrentSession() else { return }
        operationGeneration &+= 1
        let identity = RequestIdentity(
            sessionRevision: sessionRevision,
            libraryId: library.id,
            parentScope: parentScope,
            generation: operationGeneration
        )
        activeRequestIdentity = identity
        let previouslyLoaded = state.visibleNodes

        if isRefresh, let previouslyLoaded {
            state = .refreshing(previouslyLoaded)
        } else {
            state = .loading
        }

        guard !Task.isCancelled else {
            finishCancelled(identity: identity, previouslyLoaded: previouslyLoaded)
            return
        }

        guard let repository else {
            finishRequest(
                identity: identity,
                failure: .repositoryUnavailable,
                previouslyLoaded: previouslyLoaded
            )
            return
        }

        let repositoryTask = Task { @MainActor in
            await repository.listChildren(libraryId: library.id, parent: parentScope)
        }
        activeRepositoryTask = repositoryTask
        let result = await withTaskCancellationHandler {
            await repositoryTask.value
        } onCancel: {
            repositoryTask.cancel()
        }

        guard activeRequestIdentity == identity else { return }
        activeRepositoryTask = nil
        guard validateCurrentSession() else { return }
        if Task.isCancelled {
            finishCancelled(identity: identity, previouslyLoaded: previouslyLoaded)
            return
        }

        switch result {
        case .loaded(let nodes):
            guard isValidDirectory(nodes) else {
                finishRequest(
                    identity: identity,
                    failure: .node(.protocolFailure),
                    previouslyLoaded: previouslyLoaded
                )
                return
            }
            activeRequestIdentity = nil
            let ordered = Self.presentationOrder(nodes)
            state = ordered.isEmpty ? .empty : .loaded(ordered)
        case .failed(let failure):
            finishRequest(
                identity: identity,
                failure: .node(failure),
                previouslyLoaded: previouslyLoaded
            )
        }
    }

    private func finishRequest(
        identity: RequestIdentity,
        failure: NodeBrowserUIFailure,
        previouslyLoaded: [Node]?
    ) {
        guard activeRequestIdentity == identity else { return }
        guard validateCurrentSession() else { return }

        if case .node(.cancelled) = failure {
            finishCancelled(identity: identity, previouslyLoaded: previouslyLoaded)
            return
        }
        activeRequestIdentity = nil
        if let previouslyLoaded, failure.mayRetainPreviouslyLoadedData {
            state = .refreshFailed(previouslyLoaded, failure)
        } else {
            state = .failed(failure)
        }
    }

    private func finishCancelled(identity: RequestIdentity, previouslyLoaded: [Node]?) {
        guard activeRequestIdentity == identity else { return }
        activeRequestIdentity = nil
        guard validateCurrentSession() else { return }
        if let previouslyLoaded {
            state = .refreshCancelled(previouslyLoaded)
        } else {
            state = .cancelled
        }
    }

    static func presentationOrder(_ nodes: [Node]) -> [Node] {
        nodes.sorted {
            if $0.kind != $1.kind { return $0.kind == .directory }
            let comparison = $0.name.compare(
                $1.name,
                options: [.caseInsensitive, .numeric],
                locale: Locale(identifier: "en_US_POSIX")
            )
            if comparison != .orderedSame { return comparison == .orderedAscending }
            return $0.id.rawValue < $1.id.rawValue
        }
    }

    private func isValidDirectory(_ nodes: [Node]) -> Bool {
        var identifiers: Set<NodeId> = []
        return nodes.allSatisfy { node in
            node.libraryId == library.id
                && node.parentId == parentScope.expectedParentId
                && node.id != parentScope.expectedParentId
                && node.state == .active
                && node.trashedAt == nil
                && node.restoreDeadline == nil
                && identifiers.insert(node.id).inserted
        }
    }

    private struct RequestIdentity: Equatable {
        let sessionRevision: UInt64
        let libraryId: LibraryId
        let parentScope: NodeParentScope
        let generation: UInt64
    }
}
