import Foundation
import Observation

struct MetadataMutationNotice: Equatable {
    let title: String
    let message: String
    let symbol: String
    let isError: Bool
}

enum MutationQueueStatePresentation {
    static func label(for state: MutationQueueState) -> String {
        switch state {
        case .pending: "Queued"
        case .submitting: "Sending"
        case .applied: "Applied"
        case .conflict: "Conflict — review needed"
        case .outcomeUnknown: "Outcome unknown"
        case .blockedRebaseline: "Sync recovery required"
        case .failedPermanent: "Could not apply"
        }
    }

    static func symbol(for state: MutationQueueState) -> String {
        switch state {
        case .pending: "tray.full"
        case .submitting: "arrow.up.circle"
        case .applied: "checkmark.circle"
        case .conflict: "exclamationmark.bubble"
        case .outcomeUnknown: "questionmark.circle"
        case .blockedRebaseline: "arrow.triangle.2.circlepath"
        case .failedPermanent: "xmark.circle"
        }
    }
}

enum MutationConflictPresentation {
    static func message(for reason: ClientMutationConflictReason?) -> String {
        guard let reason else {
            return
                "The server kept its current metadata. Review the Library before preparing a new change."
        }
        let explanation: String
        switch reason {
        case .revisionMismatch:
            explanation =
                "The item revision changed. Refresh its folder before preparing a new change."
        case .nodeStateChanged:
            explanation =
                "The item state changed on the server. Refresh the Library before preparing another change."
        case .parentChanged:
            explanation =
                "The item’s parent changed on the server. Refresh its folder before preparing a new change."
        case .nameOccupied:
            explanation =
                "That name is already in use. Choose a new name only after reviewing the current folder."
        case .destinationChanged:
            explanation =
                "The destination changed on the server. Refresh the destination before preparing another move."
        case .resourcePurged:
            explanation =
                "The server reports that this resource was purged. No automatic resolution is available."
        }
        return "\(reason.rawValue): \(explanation)"
    }
}

@Observable
@MainActor
final class MetadataMutationViewModel {
    private(set) var availability: MetadataMutationAvailability = .unavailable(.queueUnavailable)
    private(set) var folderParentAvailability: MetadataMutationAvailability = .unavailable(
        .invalidMetadata)
    private(set) var notice: MetadataMutationNotice?
    private(set) var isCheckingAvailability = false
    private(set) var isCheckingFolderParent = false
    private(set) var isPreparingChanges = false
    private(set) var isEnqueueing = false

    private let route: NodeBrowserRoute
    private let feature: (any MetadataMutationFeatureProtocol)?
    private let sessionController: SessionController
    private let sessionRevision: UInt64
    @ObservationIgnored private var operationGeneration: UInt64 = 0
    @ObservationIgnored private var invalidated = false

    init(
        route: NodeBrowserRoute,
        feature: (any MetadataMutationFeatureProtocol)?,
        sessionController: SessionController
    ) {
        self.route = route
        self.feature = feature
        self.sessionController = sessionController
        sessionRevision = sessionController.lifecycleRevision
        if feature == nil { availability = .unavailable(.queueUnavailable) }
    }

    var library: MetadataMutationLibraryContext { route.library.mutationContext }

    var canEdit: Bool {
        !invalidated && isCurrentSession && !isCheckingAvailability
            && availability == .ready && feature != nil
            && !isEnqueueing && !isPreparingChanges
    }

    var canCreateFolder: Bool {
        canEdit && !isCheckingFolderParent && folderParentAvailability == .ready
    }

    var setupMessage: String? {
        guard !invalidated else { return nil }
        switch availability {
        case .ready: nil
        case .setupRequired:
            "Enable Changes connects to the server to establish synchronization state."
        case .unavailable(let reason): Self.message(for: reason)
        }
    }

    func refreshAvailability(forceParentRefresh: Bool = false) async {
        guard validateSession(), !isCheckingAvailability,
            let feature
        else { return }
        operationGeneration &+= 1
        let generation = operationGeneration
        isCheckingAvailability = true
        availability = await feature.availability(in: library)
        guard isCurrent(generation) else { return }
        if availability == .ready {
            isCheckingFolderParent = true
            folderParentAvailability = await feature.folderParentAvailability(
                in: library,
                parentNodeId: route.parentScope.expectedParentId,
                parentSnapshot: route.parentNodeSnapshot,
                forceRefresh: forceParentRefresh)
            guard isCurrent(generation) else { return }
            isCheckingFolderParent = false
        } else {
            isCheckingFolderParent = false
            folderParentAvailability = .unavailable(.invalidMetadata)
        }
        isCheckingAvailability = false
    }

    func enableChanges() async {
        guard validateSession(), !isPreparingChanges, let feature else { return }
        guard route.library.status == .active else {
            availability = .unavailable(
                route.library.status == .readOnly ? .libraryReadOnly : .libraryQuarantined)
            return
        }
        operationGeneration &+= 1
        let generation = operationGeneration
        isPreparingChanges = true
        notice = nil
        let result = await feature.enableChanges(in: library)
        guard isCurrent(generation) else { return }
        isPreparingChanges = false
        switch result {
        case .prepared:
            availability = .ready
            notice = MetadataMutationNotice(
                title: "Changes enabled",
                message: "Synchronization state is ready. No metadata change was sent.",
                symbol: "checkmark.circle", isError: false)
        case .alreadyPrepared:
            availability = .ready
            notice = MetadataMutationNotice(
                title: "Changes already enabled",
                message: "The verified synchronization base is ready. No metadata change was sent.",
                symbol: "checkmark.circle", isError: false)
        case .failed(let failure):
            notice = MetadataMutationNotice(
                title: "Could not enable changes", message: Self.message(for: failure),
                symbol: "exclamationmark.triangle", isError: true)
        }
        if result == .prepared || result == .alreadyPrepared {
            await refreshAvailability()
        }
    }

    func enqueueFolder(name: String) async -> Bool {
        guard folderParentAvailability == .ready else { return false }
        await enqueue(
            .createFolder(
                parentNodeId: route.parentScope.expectedParentId,
                parentAncestry: route.ancestry,
                parentSnapshot: route.parentNodeSnapshot, name: name))
    }

    func enqueueRename(node: Node, newName: String) async -> Bool {
        await enqueue(.rename(node: node, newName: newName))
    }

    func enqueueMove(node: Node, destination: Node, destinationAncestry: [NodeId]) async -> Bool {
        await enqueue(
            .move(node: node, destination: destination, destinationAncestry: destinationAncestry))
    }

    func enqueueTrash(node: Node, confirmed: Bool) async -> Bool {
        await enqueue(.trash(node: node, confirmed: confirmed))
    }

    func sessionDidChange() {
        guard !isCurrentSession else { return }
        invalidate()
    }

    func invalidate() {
        guard !invalidated else { return }
        invalidated = true
        operationGeneration &+= 1
        isCheckingAvailability = false
        isCheckingFolderParent = false
        isPreparingChanges = false
        isEnqueueing = false
        notice = nil
        availability = .unavailable(.sessionUnavailable)
    }

    private func enqueue(_ command: MetadataMutationCommand) async -> Bool {
        guard validateSession(), !isEnqueueing, let feature else { return false }
        guard route.library.status == .active, availability == .ready else {
            if route.library.status != .active {
                availability = .unavailable(
                    route.library.status == .readOnly ? .libraryReadOnly : .libraryQuarantined)
            }
            return false
        }
        operationGeneration &+= 1
        let generation = operationGeneration
        isEnqueueing = true
        notice = nil
        let result = await feature.enqueue(command, in: library)
        guard isCurrent(generation) else { return false }
        isEnqueueing = false
        switch result {
        case .persisted(let receipt):
            notice = Self.notice(for: receipt)
            return true
        case .failed(let failure):
            notice = MetadataMutationNotice(
                title: "Change not queued", message: Self.message(for: failure),
                symbol: "exclamationmark.triangle", isError: true)
            return false
        }
    }

    private var isCurrentSession: Bool {
        !invalidated && sessionController.state == .authenticated
            && sessionController.lifecycleRevision == sessionRevision
    }

    private func validateSession() -> Bool {
        guard isCurrentSession else {
            invalidate()
            return false
        }
        return true
    }

    private func isCurrent(_ generation: UInt64) -> Bool {
        guard generation == operationGeneration, validateSession() else { return false }
        return true
    }

    private static func notice(for receipt: MetadataMutationReceipt) -> MetadataMutationNotice {
        switch receipt.state {
        case .pending:
            MetadataMutationNotice(
                title: "Queued",
                message:
                    "This change is saved on this device. The server has not changed. Send it from Pending Changes.",
                symbol: "tray.full", isError: false)
        case .submitting:
            MetadataMutationNotice(
                title: "Sending", message: "This operation is already being submitted.",
                symbol: "arrow.up.circle", isError: false)
        case .applied:
            MetadataMutationNotice(
                title: "Applied",
                message: "The server confirmed this change and its result was saved.",
                symbol: "checkmark.circle", isError: false)
        case .conflict:
            MetadataMutationNotice(
                title: "Conflict — review needed",
                message:
                    "The server kept its current metadata. Review the conflict before preparing another change.",
                symbol: "exclamationmark.bubble", isError: true)
        case .outcomeUnknown:
            MetadataMutationNotice(
                title: "Outcome unknown",
                message:
                    "The server may have processed this change, but Synveil could not confirm it. Check Pending Changes before retrying.",
                symbol: "questionmark.circle", isError: true)
        case .blockedRebaseline:
            MetadataMutationNotice(
                title: "Synchronization recovery required",
                message:
                    "The synchronization base must be reconciled before more changes can be sent.",
                symbol: "arrow.trianglehead.2.clockwise", isError: true)
        case .failedPermanent:
            MetadataMutationNotice(
                title: "Could not apply",
                message:
                    "The server rejected this operation. It remains in Pending Changes for review.",
                symbol: "xmark.circle", isError: true)
        }
    }

    static func message(for reason: MetadataMutationUnavailableReason) -> String {
        switch reason {
        case .libraryReadOnly: "This Library is read-only. Browsing remains available."
        case .libraryQuarantined: "This Library is quarantined. Browsing remains available."
        case .sessionUnavailable:
            "The authenticated session changed. Reopen the Library to continue."
        case .queueUnavailable:
            "The durable change queue is unavailable. Browsing remains available."
        case .recoveryRequired: "Synchronization state needs recovery before changes can be sent."
        case .invalidMetadata:
            "Required authoritative metadata is unavailable. Refresh this folder and try again."
        }
    }

    static func message(for failure: MetadataMutationFailure) -> String {
        switch failure {
        case .readOnlyLibrary: "This Library is read-only. Browsing remains available."
        case .quarantinedLibrary: "This Library is quarantined. Browsing remains available."
        case .sessionUnavailable:
            "The authenticated session changed. Reopen the Library before continuing."
        case .queueUnavailable:
            "The durable change queue is unavailable. No server change was confirmed."
        case .syncBaseRequired: "Enable Changes for This Library before preparing an operation."
        case .recoveryRequired:
            "Synchronization state needs recovery before more changes can be sent."
        case .invalidMetadata:
            "Required authoritative metadata is missing or no longer valid. Refresh and try again."
        case .invalidName: "Enter a valid logical name. Its exact Unicode spelling is preserved."
        case .confirmationRequired: "Confirm the selected item before moving it to Trash."
        case .duplicateSubmission:
            "This change is already being saved. Wait for its status to appear."
        case .operationUnavailable:
            "This operation no longer has the authoritative metadata needed to continue."
        case .retryLimitReached:
            "The eight recovery attempts are used. Keep the operation history and contact the server owner for recovery guidance."
        case .commitAcknowledgementUncertain:
            "The local save acknowledgement was lost. Check Pending Changes before trying again; this does not mean the operation was rolled back."
        case .resultPersistenceUncertain:
            "The server result could not be saved locally. Check Pending Changes before any retry."
        case .conflictNeedsReview:
            "The server reports a conflict. Review its reason before preparing a new change."
        case .unknownNeedsReconciliation:
            "The server may already have processed this operation. Check its original outcome before any new change."
        case .permanentRejection:
            "The server permanently rejected this operation. It cannot be retried unconditionally."
        case .offline:
            "Synveil is offline. The synchronization setup did not complete; try again when connected."
        case .authorizationRequired:
            "The server did not authorize synchronization setup. Review the current session before trying again."
        case .serverUnavailable:
            "The server is temporarily unavailable. Synchronization setup can be tried again later."
        case .transportUnavailable:
            "The server result could not be confirmed. Review the durable status before retrying."
        }
    }
}

enum MutationActivityViewState: Equatable {
    case idle
    case loading
    case loaded([MetadataMutationActivityItem])
    case failed(MetadataMutationFailure)
    case invalidated
}

@Observable
@MainActor
final class MutationActivityViewModel {
    private(set) var state: MutationActivityViewState = .idle
    private(set) var availability: MetadataMutationAvailability = .unavailable(.queueUnavailable)
    private(set) var isSending = false
    private(set) var isPreparingChanges = false
    private(set) var isReconciling = false
    private(set) var isRestoring = false
    private(set) var notice: MetadataMutationNotice?
    private(set) var restoreReadyIDs: Set<String> = []
    private(set) var dataRefreshGeneration: UInt64 = 0

    let library: MetadataMutationLibraryContext
    private let feature: any MetadataMutationFeatureProtocol
    private let sessionController: SessionController
    private let sessionRevision: UInt64
    @ObservationIgnored private var generation: UInt64 = 0
    @ObservationIgnored private var invalidated = false

    init(
        library: MetadataMutationLibraryContext,
        feature: any MetadataMutationFeatureProtocol,
        sessionController: SessionController
    ) {
        self.library = library
        self.feature = feature
        self.sessionController = sessionController
        sessionRevision = sessionController.lifecycleRevision
    }

    var items: [MetadataMutationActivityItem] {
        guard isCurrentSession, case .loaded(let values) = state else { return [] }
        return values
    }

    func load() async {
        guard validateSession(), state != .loading, !isSending, !isPreparingChanges,
            !isReconciling, !isRestoring
        else { return }
        generation &+= 1
        let currentGeneration = generation
        state = .loading
        availability = await feature.availability(in: library)
        guard isCurrent(currentGeneration) else { return }
        switch await feature.activity(in: library, limit: MutationQueuePolicy.maximumReadBatch) {
        case .loaded(let values):
            guard isCurrent(currentGeneration) else { return }
            state = .loaded(values)
        case .failed(let failure):
            guard isCurrent(currentGeneration) else { return }
            state = .failed(failure)
        }
    }

    func enableChanges() async {
        guard validateSession(), !isPreparingChanges, !isSending, !isReconciling,
            !isRestoring, library.status == .active
        else { return }
        generation &+= 1
        let currentGeneration = generation
        isPreparingChanges = true
        notice = nil
        let result = await feature.enableChanges(in: library)
        guard isCurrent(currentGeneration) else { return }
        isPreparingChanges = false
        switch result {
        case .prepared:
            availability = .ready
            notice = MetadataMutationNotice(
                title: "Changes enabled",
                message: "Synchronization state is ready. No metadata change was sent.",
                symbol: "checkmark.circle", isError: false)
        case .alreadyPrepared:
            availability = .ready
            notice = MetadataMutationNotice(
                title: "Changes already enabled",
                message: "The verified synchronization base is ready. No metadata change was sent.",
                symbol: "checkmark.circle", isError: false)
        case .failed(let failure):
            notice = MetadataMutationNotice(
                title: "Could not enable changes",
                message: MetadataMutationViewModel.message(for: failure),
                symbol: "exclamationmark.triangle", isError: true)
        }
        await load()
    }

    func sendPendingChanges() async {
        guard validateSession(), !isSending, !isReconciling, !isRestoring,
            !isPreparingChanges, availability == .ready, library.status == .active
        else { return }
        generation &+= 1
        let currentGeneration = generation
        isSending = true
        notice = nil
        let result = await feature.sendPendingChanges(in: library)
        guard isCurrent(currentGeneration) else { return }
        isSending = false
        if result.applied > 0 { dataRefreshGeneration &+= 1 }
        notice = Self.notice(for: result)
        await load()
    }

    func reconcileUnknown(mutationId: String) async {
        guard validateSession(), !isReconciling, !isSending, !isRestoring,
            !isPreparingChanges, library.status == .active,
            availability == .ready,
            let item = items.first(where: { $0.id == mutationId }), item.state == .outcomeUnknown,
            let retries = item.recoveryAttemptCount,
            retries < MutationQueuePolicy.maximumRecoveryAttempts
        else {
            notice = MetadataMutationNotice(
                title: "Retry unavailable",
                message:
                    "The current session, queue status, or recovery limit does not allow another check.",
                symbol: "exclamationmark.triangle", isError: true)
            return
        }
        generation &+= 1
        let currentGeneration = generation
        isReconciling = true
        let result = await feature.reconcileUnknown(mutationId: mutationId, in: library)
        guard isCurrent(currentGeneration) else { return }
        isReconciling = false
        if result.applied > 0 { dataRefreshGeneration &+= 1 }
        notice = Self.notice(for: result)
        await load()
    }

    func checkRestore(operationId: String) async -> Bool {
        guard validateSession(), !isRestoring, !isSending, !isReconciling,
            !isPreparingChanges, library.status == .active,
            availability == .ready,
            items.contains(where: { $0.id == operationId && $0.mayCheckRestore })
        else { return false }
        generation &+= 1
        let currentGeneration = generation
        isRestoring = true
        notice = nil
        let failure = await feature.checkRestore(trashedOperationId: operationId, in: library)
        guard isCurrent(currentGeneration) else { return false }
        isRestoring = false
        if let failure {
            restoreReadyIDs.remove(operationId)
            notice = MetadataMutationNotice(
                title: "Restore unavailable",
                message: MetadataMutationViewModel.message(for: failure),
                symbol: "exclamationmark.triangle", isError: true)
            return false
        }
        restoreReadyIDs.insert(operationId)
        notice = MetadataMutationNotice(
            title: "Restore is ready",
            message:
                "The original parent is active. Confirm to queue a restore using its current revision.",
            symbol: "checkmark.circle", isError: false)
        return true
    }

    func enqueueRestore(operationId: String) async {
        guard validateSession(), !isRestoring, !isSending, !isReconciling,
            !isPreparingChanges, library.status == .active,
            availability == .ready,
            restoreReadyIDs.contains(operationId)
        else { return }
        generation &+= 1
        let currentGeneration = generation
        isRestoring = true
        restoreReadyIDs.remove(operationId)
        let result = await feature.enqueueRestore(trashedOperationId: operationId, in: library)
        guard isCurrent(currentGeneration) else { return }
        isRestoring = false
        switch result {
        case .persisted(let receipt):
            notice = MetadataMutationViewModel.notice(for: receipt)
        case .failed(let failure):
            notice = MetadataMutationNotice(
                title: "Restore not queued",
                message: MetadataMutationViewModel.message(for: failure),
                symbol: "exclamationmark.triangle", isError: true)
        }
        await load()
    }

    func sessionDidChange() {
        guard !isCurrentSession else { return }
        invalidate()
    }

    func invalidate() {
        guard !invalidated else { return }
        invalidated = true
        generation &+= 1
        state = .invalidated
        notice = nil
        restoreReadyIDs.removeAll()
        isSending = false
        isReconciling = false
        isRestoring = false
    }

    private var isCurrentSession: Bool {
        !invalidated && sessionController.state == .authenticated
            && sessionController.lifecycleRevision == sessionRevision
    }

    private func validateSession() -> Bool {
        guard isCurrentSession else {
            invalidate()
            return false
        }
        return true
    }

    private func isCurrent(_ currentGeneration: UInt64) -> Bool {
        currentGeneration == generation && validateSession()
    }

    private static func notice(for result: MetadataMutationDrainPresentation)
        -> MetadataMutationNotice
    {
        var parts: [String] = []
        if result.applied > 0 {
            parts.append("\(result.applied) \(result.applied == 1 ? "change" : "changes") applied")
        }
        if result.conflicts > 0 {
            parts.append(
                "\(result.conflicts) \(result.conflicts == 1 ? "change needs" : "changes need") conflict review"
            )
        }
        if result.unknown > 0 {
            parts.append(
                "\(result.unknown) \(result.unknown == 1 ? "change has" : "changes have") an unknown outcome"
            )
        }
        if result.permanentRejections > 0 {
            parts.append(
                "\(result.permanentRejections) \(result.permanentRejections == 1 ? "change could" : "changes could") not be applied"
            )
        }
        if result.rebaselineBlocked > 0 { parts.append("Synchronization base needs recovery") }
        if result.localFailures > 0 { parts.append("A local pre-dispatch check failed") }
        if let stop = result.stop {
            parts.append(MetadataMutationViewModel.message(for: stop))
        } else if result.attempted == 0 {
            parts.append("No queued changes were available.")
        } else if result.applied > 0 {
            parts.append("Review Pending Changes for any remaining operations.")
        }
        if result.stop == .resultPersistenceUncertain {
            return MetadataMutationNotice(
                title: "Result not saved", message: parts.joined(separator: ". "),
                symbol: "externaldrive.badge.exclamationmark", isError: true)
        }
        return MetadataMutationNotice(
            title: "Pending Changes updated", message: parts.joined(separator: ". "),
            symbol: result.applied > 0 ? "checkmark.circle" : "tray.full",
            isError: result.stop != nil || result.conflicts > 0 || result.unknown > 0
                || result.permanentRejections > 0 || result.rebaselineBlocked > 0)
    }
}
