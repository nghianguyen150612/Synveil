import Foundation
import Observation

enum NodeFileDetailsState: Equatable {
    case idle
    case loading
    case loaded(Node)
    case refreshing(Node)
    case refreshFailed(Node, NodeFileDetailsFailure)
    case unavailable
    case savedUnavailable(NodeCacheUIFailure)
    case failed(NodeFileDetailsFailure)
    case cancelled
    case invalidated

    var visibleNode: Node? {
        switch self {
        case .loaded(let node), .refreshing(let node), .refreshFailed(let node, _): node
        case .idle, .loading, .unavailable, .savedUnavailable, .failed, .cancelled,
            .invalidated: nil
        }
    }

    var isRequestInProgress: Bool {
        switch self {
        case .loading, .refreshing: true
        default: false
        }
    }
}

enum NodeFileDetailsFailure: Equatable {
    case repositoryUnavailable
    case node(NodeFailure)

    var title: String {
        switch self {
        case .repositoryUnavailable: "File details unavailable"
        case .node(.protocolFailure), .node(.unexpectedContentType), .node(.repeatedCursor):
            "File metadata could not be verified"
        case .node(.resourceLimit): "File metadata exceeds the safety limit"
        case .node(.httpFailure): "File metadata request failed"
        case .node(.cancelled): "File metadata request cancelled"
        case .node(let failure): NodeBrowserUIFailure.node(failure).title
        }
    }

    var message: String {
        switch self {
        case .repositoryUnavailable:
            "Secure file access is not ready. Reopen Synveil before viewing file details."
        case .node(.protocolFailure), .node(.unexpectedContentType), .node(.repeatedCursor):
            "The server response did not match the expected file metadata. Contact the server owner."
        case .node(.resourceLimit):
            "The file metadata response exceeds the client safety limit. Contact the server owner."
        case .node(.unauthenticated):
            "This session is no longer available. Return to sign in before viewing file details."
        case .node(.originMismatch):
            "This file could not be matched to the configured server. Its metadata is hidden for safety."
        case .node(.staleSession):
            "The device session changed while loading. Previously displayed metadata was cleared."
        case .node(.cancelled):
            "The file metadata request was cancelled. You can refresh when ready."
        case .node(.offline):
            "Check your internet connection, then retry this read-only metadata request."
        case .node(let failure):
            NodeBrowserUIFailure.node(failure).message
        }
    }

    var canRetry: Bool {
        guard case .node(let failure) = self else { return false }
        return NodeBrowserUIFailure.node(failure).canRetry
    }

    var mayRetainPreviouslyLoadedData: Bool {
        guard case .node(let failure) = self else { return false }
        return NodeBrowserUIFailure.node(failure).mayRetainPreviouslyLoadedData
    }
}

/// Owns one immutable file selection and transient metadata for one authenticated lifecycle.
@Observable
@MainActor
final class NodeFileDetailsViewModel {
    private(set) var state: NodeFileDetailsState = .idle
    private(set) var contentSource: NodeBrowserContentSource
    private(set) var savedProjectionState: NodeProjectionState?
    let route: NodeFileDetailsRoute

    private let repository: (any NodeRepositoryProtocol)?
    private let offlineDetailsService: (any OfflineNodeBrowserServiceProtocol)?
    private let sessionController: SessionController
    private let sessionRevision: UInt64
    @ObservationIgnored private var generation: UInt64 = 0
    @ObservationIgnored private var activeGeneration: UInt64?
    @ObservationIgnored private var activeTask: Task<NodeDetailsRepositoryResult, Never>?
    @ObservationIgnored private var activeSavedTask: Task<OfflineNodeDetailsResult, Never>?
    @ObservationIgnored private var invalidated = false
    @ObservationIgnored private var didLoad = false

    init(
        repository: (any NodeRepositoryProtocol)?, sessionController: SessionController,
        offlineDetailsService: (any OfflineNodeBrowserServiceProtocol)? = nil,
        route: NodeFileDetailsRoute
    ) {
        self.repository = repository
        self.sessionController = sessionController
        self.offlineDetailsService = offlineDetailsService
        self.route = route
        contentSource = route.contentSource
        sessionRevision = sessionController.lifecycleRevision
    }

    /// Guards rendering even before SwiftUI's lifecycle observer clears the stored state.
    var presentationState: NodeFileDetailsState { isCurrentSession ? state : .invalidated }
    var visibleNode: Node? { presentationState.visibleNode }
    var isRequestInProgress: Bool { presentationState.isRequestInProgress }
    var canRefresh: Bool {
        guard isCurrentSession, activeGeneration == nil else { return false }
        return switch state {
        case .failed(let failure), .refreshFailed(_, let failure): failure.canRetry
        case .loading, .refreshing, .invalidated: false
        case .idle, .loaded, .unavailable, .savedUnavailable, .cancelled: true
        }
    }

    func loadIfNeeded() async {
        guard !didLoad, case .idle = state else { return }
        didLoad = true
        if route.contentSource == .cached {
            await requestSaved()
        } else {
            await requestLive()
        }
    }

    func refresh() async {
        guard canRefresh else { return }
        didLoad = true
        await requestLive()
    }

    func sessionDidChange() {
        if !isCurrentSession { invalidate() }
    }

    func invalidate() {
        guard !invalidated else { return }
        invalidated = true
        generation &+= 1
        activeGeneration = nil
        activeTask?.cancel()
        activeTask = nil
        activeSavedTask?.cancel()
        activeSavedTask = nil
        savedProjectionState = nil
        state = .invalidated
    }

    private var isCurrentSession: Bool {
        !invalidated && sessionController.state == .authenticated
            && sessionController.lifecycleRevision == sessionRevision
    }

    private func requestLive() async {
        guard isCurrentSession else {
            invalidate()
            return
        }
        guard activeGeneration == nil else { return }
        generation &+= 1
        let operation = generation
        activeGeneration = operation
        let previous = state.visibleNode
        state = previous.map(NodeFileDetailsState.refreshing) ?? .loading
        guard !Task.isCancelled else {
            finish(.failed(.cancelled), operation: operation, previous: previous)
            return
        }
        guard let repository else {
            activeGeneration = nil
            state = .failed(.repositoryUnavailable)
            return
        }
        let task = Task { @MainActor in
            await repository.getNode(
                libraryId: route.library.id, nodeId: route.nodeId, expectedParent: route.parentScope
            )
        }
        activeTask = task
        let result = await withTaskCancellationHandler {
            await task.value
        } onCancel: {
            task.cancel()
        }
        guard activeGeneration == operation else { return }
        activeTask = nil
        finish(
            Task.isCancelled ? .failed(.cancelled) : result, operation: operation,
            previous: previous)
    }

    private func requestSaved() async {
        guard isCurrentSession else {
            invalidate()
            return
        }
        guard activeGeneration == nil else { return }
        generation &+= 1
        let operation = generation
        activeGeneration = operation
        state = .loading
        guard let offlineDetailsService else {
            activeGeneration = nil
            state = .savedUnavailable(.unavailable)
            return
        }
        let task = Task { @MainActor in
            await offlineDetailsService.savedNode(
                libraryId: route.library.id, nodeId: route.nodeId,
                expectedParent: route.parentScope, ancestry: route.ancestry)
        }
        activeSavedTask = task
        let result = await withTaskCancellationHandler {
            await task.value
        } onCancel: {
            task.cancel()
        }
        guard activeGeneration == operation else { return }
        activeSavedTask = nil
        guard isCurrentSession else {
            invalidate()
            return
        }
        if Task.isCancelled {
            activeGeneration = nil
            state = .cancelled
            return
        }
        activeGeneration = nil
        switch result {
        case .unavailable(let failure):
            savedProjectionState = nil
            state = .savedUnavailable(failure)
        case .loaded(let node, let projection):
            guard node.id == route.nodeId, node.libraryId == route.library.id,
                node.parentId == route.parentScope.expectedParentId,
                node.id != route.parentScope.expectedParentId,
                node.kind == .file, node.state == .active,
                node.trashedAt == nil, node.restoreDeadline == nil, !node.purgeEligible
            else {
                state = .savedUnavailable(.unavailable)
                return
            }
            contentSource = .cached
            savedProjectionState = projection
            state = .loaded(node)
        }
    }

    private func finish(_ result: NodeDetailsRepositoryResult, operation: UInt64, previous: Node?) {
        guard activeGeneration == operation else { return }
        guard isCurrentSession else {
            invalidate()
            return
        }
        activeGeneration = nil
        switch result {
        case .loaded(let node):
            // Defense in depth for injected repositories: no wrong file, Library, or parent escapes.
            guard node.id == route.nodeId, node.libraryId == route.library.id,
                node.parentId == route.parentScope.expectedParentId,
                node.id != route.parentScope.expectedParentId,
                node.kind == .file, node.state == .active,
                node.trashedAt == nil, node.restoreDeadline == nil, !node.purgeEligible
            else {
                state = .unavailable
                return
            }
            state = .loaded(node)
            contentSource = .live
            savedProjectionState = nil
        case .unavailable, .inconsistent:
            savedProjectionState = nil
            state = .unavailable
        case .failed(.cancelled):
            state = .cancelled
        case .failed(let failure):
            let displayFailure = NodeFileDetailsFailure.node(failure)
            if let previous, displayFailure.mayRetainPreviouslyLoadedData {
                state = .refreshFailed(previous, displayFailure)
                if contentSource != .cached { contentSource = .previouslyLoaded }
            } else {
                savedProjectionState = nil
                state = .failed(displayFailure)
            }
        }
    }
}
