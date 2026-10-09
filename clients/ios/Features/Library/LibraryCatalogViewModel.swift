import Foundation
import Observation

/// A transient screen state. Catalog values only live while this authenticated presentation lives.
enum LibraryCatalogViewState: Equatable {
    case idle
    case loading
    case loaded([Library])
    case empty
    case refreshing([Library])
    case refreshFailed([Library], LibraryCatalogUIFailure)
    case refreshCancelled([Library])
    case failed(LibraryCatalogUIFailure)
    case cancelled
    case invalidated

    var visibleLibraries: [Library]? {
        switch self {
        case .loaded(let libraries), .refreshing(let libraries),
            .refreshFailed(let libraries, _), .refreshCancelled(let libraries):
            return libraries
        case .idle, .loading, .empty, .failed, .cancelled, .invalidated:
            return nil
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

/// Safe, typed UI errors. Server bodies, headers, URLs, and device credentials never enter here.
enum LibraryCatalogUIFailure: Equatable {
    case repositoryUnavailable
    case library(LibraryFailure)

    var title: String {
        switch self {
        case .repositoryUnavailable: "Library catalog unavailable"
        case .library(let failure):
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
            case .httpFailure: "Library request failed"
            case .unexpectedContentType, .protocolFailure, .repeatedCursor:
                "Server response could not be verified"
            case .resourceLimit: "Library catalog is too large"
            case .cancelled: "Library request cancelled"
            }
        }
    }

    var message: String {
        switch self {
        case .repositoryUnavailable:
            "Secure catalog access is not ready. Reopen Synveil before viewing libraries."
        case .library(let failure):
            switch failure {
            case .unauthenticated:
                "This session is no longer available. Return to sign in before viewing libraries."
            case .credentialUnavailable, .invalidCredential:
                "Synveil could not verify this device's saved authorization. Use the existing session recovery flow."
            case .originMismatch:
                "The catalog request could not be matched to this server. The library list is hidden for safety."
            case .staleSession:
                "The device session changed while loading. The previous catalog has been cleared."
            case .authenticationRejected:
                "The server rejected this device session. Synveil is opening the recovery flow."
            case .deviceRevoked:
                "This device's access was revoked. Synveil is opening the recovery flow."
            case .offline:
                "Check your internet connection, then retry the catalog request."
            case .dnsFailure:
                "Check the network and configured server address, then retry."
            case .timeout:
                "The server did not respond in time. Check your connection and retry."
            case .tlsFailure:
                "Synveil could not verify the server's secure connection. Contact the server owner."
            case .redirectRejected:
                "The server returned a redirect that Synveil could not safely follow. Contact the server owner."
            case .serverUnavailable:
                "The server is temporarily unavailable. Wait a moment, then retry."
            case .httpFailure(let statusCode):
                "The server could not complete this request (HTTP \(statusCode))."
            case .unexpectedContentType, .protocolFailure, .repeatedCursor:
                "The server response did not match the expected library catalog format. Contact the server owner."
            case .resourceLimit:
                "The catalog exceeded the client safety limit. Contact the server owner."
            case .cancelled:
                "The catalog request was cancelled. You can retry when ready."
            }
        }
    }

    var accessibilityDescription: String { "\(title). \(message)" }

    var canRetry: Bool {
        switch self {
        case .repositoryUnavailable: false
        case .library(let failure):
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

    /// Prior results remain visible only for failures that do not cast doubt on session identity.
    var mayRetainPreviouslyLoadedData: Bool {
        guard case .library(let failure) = self else { return false }
        return switch failure {
        case .offline, .dnsFailure, .timeout, .serverUnavailable:
            true
        case .unauthenticated, .credentialUnavailable, .invalidCredential, .originMismatch,
            .staleSession, .authenticationRejected, .deviceRevoked, .tlsFailure,
            .redirectRejected, .httpFailure, .unexpectedContentType, .protocolFailure,
            .resourceLimit, .repeatedCursor, .cancelled:
            false
        }
    }
}

struct LibraryStatusPresentation: Equatable {
    let label: String
    let symbol: String

    static func make(for status: LibraryStatus) -> Self {
        switch status {
        case .active: Self(label: "Active", symbol: "checkmark.circle.fill")
        case .readOnly: Self(label: "Read Only", symbol: "lock.fill")
        case .quarantined: Self(label: "Quarantined", symbol: "exclamationmark.shield.fill")
        }
    }
}

/// Owns one authenticated catalog presentation and fences results to its session lifecycle.
@Observable
@MainActor
final class LibraryCatalogViewModel {
    private(set) var state: LibraryCatalogViewState = .idle

    private let repository: (any LibraryCatalogRepositoryProtocol)?
    private let sessionController: SessionController
    private let sessionRevision: UInt64
    @ObservationIgnored private var didStartInitialLoad = false
    @ObservationIgnored private var activeRequestID: UUID?
    @ObservationIgnored private var activeRepositoryTask: Task<LibraryRepositoryResult, Never>?
    @ObservationIgnored private var invalidated = false

    init(
        repository: (any LibraryCatalogRepositoryProtocol)?,
        sessionController: SessionController
    ) {
        self.repository = repository
        self.sessionController = sessionController
        sessionRevision = sessionController.lifecycleRevision
    }

    var visibleLibraries: [Library]? {
        guard isCurrentSession else { return nil }
        return state.visibleLibraries
    }

    var isRequestInProgress: Bool { state.isRequestInProgress }

    var canRefresh: Bool {
        guard isCurrentSession else { return false }
        return switch state {
        case .failed(let failure), .refreshFailed(_, let failure): failure.canRetry
        case .loading, .refreshing, .invalidated: false
        case .idle, .loaded, .empty, .refreshCancelled, .cancelled: true
        }
    }

    /// The SwiftUI task can reappear; this gate starts the first request at most once.
    func loadIfNeeded() async {
        guard !didStartInitialLoad, case .idle = state else { return }
        guard validateCurrentSession() else { return }
        didStartInitialLoad = true
        await requestCatalog(isRefresh: false)
    }

    /// Safe read-only refresh. Concurrent attempts are suppressed while one request is active.
    func refresh() async {
        guard validateCurrentSession(), activeRequestID == nil else { return }
        didStartInitialLoad = true
        await requestCatalog(isRefresh: true)
    }

    /// Called by the view as soon as the authoritative root session changes.
    func sessionDidChange() {
        guard !isCurrentSession else { return }
        invalidate()
    }

    /// Clears this in-memory presentation. Late results cannot repopulate an invalidated model.
    func invalidate() {
        guard !invalidated else { return }
        invalidated = true
        activeRequestID = nil
        activeRepositoryTask?.cancel()
        activeRepositoryTask = nil
        state = .invalidated
    }

    func library(with id: LibraryId) -> Library? {
        guard isCurrentSession else { return nil }
        return state.visibleLibraries?.first { $0.id == id }
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

    private func requestCatalog(isRefresh: Bool) async {
        guard activeRequestID == nil, validateCurrentSession() else { return }
        let requestID = UUID()
        activeRequestID = requestID
        let previouslyLoaded =
            state.visibleLibraries?.isEmpty == false
            ? state.visibleLibraries
            : nil

        if isRefresh, let previouslyLoaded {
            state = .refreshing(previouslyLoaded)
        } else {
            state = .loading
        }

        guard !Task.isCancelled else {
            activeRequestID = nil
            finishCancelled(previouslyLoaded: previouslyLoaded)
            return
        }

        guard let repository else {
            finishRequest(
                requestID,
                failure: .repositoryUnavailable,
                previouslyLoaded: previouslyLoaded
            )
            return
        }

        let repositoryTask = Task { @MainActor in
            await repository.listLibraries()
        }
        activeRepositoryTask = repositoryTask
        let result = await withTaskCancellationHandler {
            await repositoryTask.value
        } onCancel: {
            repositoryTask.cancel()
        }
        guard activeRequestID == requestID else { return }
        activeRepositoryTask = nil

        guard validateCurrentSession() else { return }
        if Task.isCancelled {
            activeRequestID = nil
            finishCancelled(previouslyLoaded: previouslyLoaded)
            return
        }

        switch result {
        case .loaded(let libraries):
            activeRequestID = nil
            let ordered = Self.presentationOrder(libraries)
            state = ordered.isEmpty ? .empty : .loaded(ordered)
        case .failed(let failure):
            finishRequest(
                requestID,
                failure: .library(failure),
                previouslyLoaded: previouslyLoaded
            )
        }
    }

    private func finishRequest(
        _ requestID: UUID,
        failure: LibraryCatalogUIFailure,
        previouslyLoaded: [Library]?
    ) {
        guard activeRequestID == requestID else { return }
        activeRequestID = nil
        guard validateCurrentSession() else { return }

        if case .library(.cancelled) = failure {
            finishCancelled(previouslyLoaded: previouslyLoaded)
        } else if let previouslyLoaded, failure.mayRetainPreviouslyLoadedData {
            state = .refreshFailed(previouslyLoaded, failure)
        } else {
            state = .failed(failure)
        }
    }

    private func finishCancelled(previouslyLoaded: [Library]?) {
        if let previouslyLoaded {
            state = .refreshCancelled(previouslyLoaded)
        } else {
            state = .cancelled
        }
    }

    private static func presentationOrder(_ libraries: [Library]) -> [Library] {
        libraries.sorted {
            let comparison = $0.name.compare(
                $1.name,
                options: [.caseInsensitive, .numeric],
                locale: Locale(identifier: "en_US_POSIX")
            )
            if comparison != .orderedSame { return comparison == .orderedAscending }
            return $0.id.rawValue < $1.id.rawValue
        }
    }
}
