import Foundation
import Observation
import UniformTypeIdentifiers

@Observable
@MainActor
public final class PhotoBackupSettingsViewModel {
    public var isOptedIn: Bool = false
    public var scope: PhotoBackupScope = .allAuthorized
    public var includeVideos: Bool = true
    public var pauseDiscovery: Bool = false
    public var permissionState: PhotoPermissionState = .notDetermined
    public var currentStatusText: String = "Disabled"
    public var isProcessing: Bool = false
    public private(set) var observations: [PhotoAssetObservation] = []
    public private(set) var resourceChoices: [PhotoResourceSelection] = []
    public private(set) var selectedAssetIdentifier: String?
    public private(set) var selectedResource: PhotoResourceSelection?
    public private(set) var extractionRecord: PhotoResourceExtractionRecord?
    public private(set) var uploadStatusText: String?
    public private(set) var failureReasonText: String?
    public var destinationLibraryIdentifier = ""
    public var destinationParentIdentifier = ""
    public var destinationFilename = ""

    private let permissionAdapter: PhotoLibraryPermissionChecking
    private let coordinator: PhotoLibraryObservationCoordinatorProtocol
    private let store: PhotoObservationStoreProtocol
    private let extractionCoordinator: PhotoResourceExtractionCoordinator?
    private let uploadHandoffCoordinator: PhotoResourceUploadHandoffCoordinator?
    private let rustBridge: (any RustBridgeProtocol)?
    private let libraryCatalog: (any LibraryCatalogRepositoryProtocol)?
    private let profileId: UUID
    private let ownerId: String
    private let sessionRevision: UInt64
    private var activeExtractionId: UUID?

    public var isPreparing: Bool { activeExtractionId != nil }
    public var canPrepareSelectedResource: Bool {
        extractionCoordinator != nil && selectedResource != nil && !isProcessing
    }
    public var canUploadSelectedResource: Bool {
        uploadHandoffCoordinator != nil
            && extractionRecord.map {
                [.verifiedReady, .handoffPending].contains($0.state)
            } == true
            && !isProcessing
    }

    init(
        profileId: UUID,
        ownerId: String,
        permissionAdapter: PhotoLibraryPermissionChecking = PhotoLibraryPermissionAdapter(),
        coordinator: PhotoLibraryObservationCoordinatorProtocol,
        store: PhotoObservationStoreProtocol,
        extractionCoordinator: PhotoResourceExtractionCoordinator? = nil,
        uploadHandoffCoordinator: PhotoResourceUploadHandoffCoordinator? = nil,
        rustBridge: (any RustBridgeProtocol)? = nil,
        libraryCatalog: (any LibraryCatalogRepositoryProtocol)? = nil,
        sessionRevision: UInt64 = 0
    ) {
        self.profileId = profileId
        self.ownerId = ownerId
        self.permissionAdapter = permissionAdapter
        self.coordinator = coordinator
        self.store = store
        self.extractionCoordinator = extractionCoordinator
        self.uploadHandoffCoordinator = uploadHandoffCoordinator
        self.rustBridge = rustBridge
        self.libraryCatalog = libraryCatalog
        self.sessionRevision = sessionRevision
    }

    public func loadSettings() async {
        isProcessing = true
        defer { isProcessing = false }

        permissionState = await permissionAdapter.currentPermissionState()
        do {
            let config = try await store.fetchConfiguration(profileId: profileId)
            self.isOptedIn = config.isOptedIn
            self.scope = config.scope
            self.includeVideos = config.includeVideos
            self.pauseDiscovery = config.pauseDiscovery
            if config.isOptedIn && !config.pauseDiscovery && permissionState.isAuthorized {
                let result = try await coordinator.startObservation(
                    profileId: profileId, ownerId: ownerId,
                    sessionRevision: sessionRevision)
                apply(result)
                observations = try await store.fetchObservations(
                    profileId: profileId, ownerId: ownerId)
            } else {
                try await coordinator.stopObservation()
                observations = config.isOptedIn
                    ? try await store.fetchObservations(profileId: profileId, ownerId: ownerId) : []
            }
            if let extractionCoordinator {
                try? await extractionCoordinator.recoverStagingAfterLaunch()
                if let records = try? await extractionCoordinator.recentExtractions(),
                    let latest = records.first
                {
                    extractionRecord = latest
                    uploadStatusText = label(for: latest.state)
                }
            }
            if !config.isOptedIn || config.pauseDiscovery || !permissionState.isAuthorized {
                updateStatusText()
            }
        } catch {
            currentStatusText = "Error loading configuration"
        }
    }

    public func requestPermission() async {
        permissionState = await permissionAdapter.requestPermission()
        if permissionState.isAuthorized && isOptedIn && !pauseDiscovery {
            await saveAndSyncCoordinator()
        } else {
            updateStatusText()
        }
    }

    public func selectAsset(_ localIdentifier: String?) {
        selectedAssetIdentifier = localIdentifier
        selectedResource = nil
        resourceChoices = []
        extractionRecord = nil
        failureReasonText = nil
        uploadStatusText = nil
    }

    public func assetLabel(_ observation: PhotoAssetObservation) -> String {
        let kind = observation.assetKind.rawValue.replacingOccurrences(of: "([A-Z])", with: " $1",
            options: .regularExpression).capitalized
        if let date = observation.creationDate {
            return "\(kind) · \(date.formatted(date: .abbreviated, time: .shortened))"
        }
        return kind
    }

    public func loadResourceChoices() async {
        guard let extractionCoordinator,
            let localIdentifier = selectedAssetIdentifier,
            let observation = observations.first(where: { $0.localIdentifier == localIdentifier })
        else {
            failureReasonText = "Select an observed photo or video first."
            return
        }
        isProcessing = true
        defer { isProcessing = false }
        do {
            resourceChoices = try await extractionCoordinator.availableSelections(
                for: observation)
            selectedResource = nil
            if resourceChoices.isEmpty {
                failureReasonText = "No supported original resource is available for this asset."
            } else {
                failureReasonText = nil
            }
        } catch let failure as PhotoResourceTransferFailure {
            failureReasonText = message(for: failure)
            resourceChoices = []
        } catch {
            failureReasonText = "Could not inspect this asset's original resources."
            resourceChoices = []
        }
    }

    public func selectResource(_ resource: PhotoResourceSelection) {
        guard resourceChoices.contains(resource) else { return }
        selectedResource = resource
        let ext = resource.uniformTypeIdentifier.flatMap {
            UTType($0)?.preferredFilenameExtension
        }
        if destinationFilename.isEmpty {
            destinationFilename = "Photo-\(Int(Date().timeIntervalSince1970))"
                + (ext.map { ".\($0)" } ?? ".media")
        }
        failureReasonText = nil
    }

    public func prepareSelectedAsset() async {
        guard let extractionCoordinator, let selectedResource,
            let observation = observations.first(where: {
                $0.localIdentifier == selectedResource.localIdentifier
            })
        else {
            failureReasonText = "Choose one original resource before preparing it."
            return
        }
        let operationId = UUID()
        activeExtractionId = operationId
        isProcessing = true
        failureReasonText = nil
        uploadStatusText = "Preparing original resource…"
        defer {
            activeExtractionId = nil
            isProcessing = false
        }
        do {
            let record = try await extractionCoordinator.prepareSelection(
                operationId: operationId, selection: selectedResource,
                observationGeneration: observation.discoveryGeneration)
            extractionRecord = record
            uploadStatusText = label(for: record.state)
        } catch let failure as PhotoResourceTransferFailure {
            failureReasonText = message(for: failure)
            uploadStatusText = nil
        } catch is CancellationError {
            failureReasonText = "Preparation was cancelled."
            uploadStatusText = nil
        } catch {
            failureReasonText = "Preparation failed and can be retried from this asset."
            uploadStatusText = nil
        }
    }

    public func cancelPreparation() {
        guard let operationId = activeExtractionId, let extractionCoordinator else { return }
        Task { await extractionCoordinator.cancelPreparation(operationId: operationId) }
    }

    public func uploadSelectedVerifiedAsset() async {
        guard let extractionRecord, extractionRecord.state == .verifiedReady,
            let uploadHandoffCoordinator, let rustBridge, let libraryCatalog
        else {
            failureReasonText = "Prepare and verify one original resource before uploading it."
            return
        }
        guard !destinationFilename.isEmpty else {
            failureReasonText = "Enter a destination filename."
            return
        }
        isProcessing = true
        defer { isProcessing = false }
        do {
            let libraryId = try await LibraryId.validated(
                destinationLibraryIdentifier, using: rustBridge)
            let parentId = try await NodeId.validated(
                destinationParentIdentifier, using: rustBridge)
            let availableLibraries = await libraryCatalog.listLibraries()
            guard case .loaded(let libraries) = availableLibraries,
                let library = libraries.first(where: { $0.id == libraryId }),
                library.status == .active
            else { throw PhotoResourceTransferFailure.staleAuthorization }
            let target = UploadTarget.createFile(
                library: libraryId, parent: parentId, name: destinationFilename)
            uploadStatusText = "Starting explicit foreground upload…"
            failureReasonText = nil
            let result = await uploadHandoffCoordinator.uploadSelected(
                extractionId: extractionRecord.operationId,
                target: target,
                libraryStatus: library.status,
                progress: { progress in
                    self.uploadStatusText = Self.uploadLabel(for: progress.state)
                })
            switch result {
            case .success(let uploadResult):
                updateUploadStatus(uploadResult)
                if case .stopped(_, let failure) = uploadResult {
                    failureReasonText = String(describing: failure)
                }
            case .failure(let failure):
                failureReasonText = message(for: failure)
                uploadStatusText = nil
            }
            if let latest = try? await uploadHandoffCoordinator.currentExtraction(
                extractionId: extractionRecord.operationId) {
                extractionRecord = latest
            }
        } catch let failure as PhotoResourceTransferFailure {
            failureReasonText = message(for: failure)
            uploadStatusText = nil
        } catch {
            failureReasonText = "The destination could not be validated."
            uploadStatusText = nil
        }
    }

    public func resumeForegroundUpload() async {
        guard let record = extractionRecord, record.state == .handedOff,
            let uploadId = record.relatedUploadOperationId,
            let uploadHandoffCoordinator, let rustBridge, let libraryCatalog
        else { return }
        isProcessing = true
        defer { isProcessing = false }
        do {
            let libraryId = try await LibraryId.validated(
                destinationLibraryIdentifier, using: rustBridge)
            let availableLibraries = await libraryCatalog.listLibraries()
            guard case .loaded(let libraries) = availableLibraries,
                let library = libraries.first(where: { $0.id == libraryId }),
                library.status == .active
            else { throw PhotoResourceTransferFailure.staleAuthorization }
            let result = await uploadHandoffCoordinator.resumeExisting(
                extractionId: record.operationId,
                uploadOperationId: uploadId,
                library: libraryId,
                libraryStatus: library.status,
                progress: { progress in
                    self.uploadStatusText = Self.uploadLabel(for: progress.state)
                })
            switch result {
            case .success(let uploadResult): updateUploadStatus(uploadResult)
            case .failure(let failure): failureReasonText = message(for: failure)
            }
            if let latest = try? await uploadHandoffCoordinator.currentExtraction(
                extractionId: record.operationId) {
                extractionRecord = latest
            }
        } catch {
            failureReasonText = "The saved foreground upload could not be resumed."
        }
    }

    public func pauseForegroundUpload() async {
        guard let uploadHandoffCoordinator, let extractionRecord else { return }
        do {
            try await uploadHandoffCoordinator.pauseForegroundUpload(
                extractionId: extractionRecord.operationId)
            uploadStatusText = "Foreground upload paused; the existing P045 operation can resume."
        } catch {
            failureReasonText = "The foreground upload could not be paused safely."
        }
    }

    public func toggleOptIn(_ enabled: Bool) async {
        isOptedIn = enabled
        await saveAndSyncCoordinator()
    }

    public func updateIncludeVideos(_ include: Bool) async {
        includeVideos = include
        await saveAndSyncCoordinator()
    }

    public func togglePauseDiscovery(_ pause: Bool) async {
        pauseDiscovery = pause
        await saveAndSyncCoordinator()
    }

    private func saveAndSyncCoordinator() async {
        let config = PhotoBackupConfiguration(
            isOptedIn: isOptedIn,
            scope: scope,
            includeVideos: includeVideos,
            pauseDiscovery: pauseDiscovery
        )
        do {
            try await store.saveConfiguration(config, profileId: profileId)
            if isOptedIn && !pauseDiscovery {
                let result = try await coordinator.startObservation(
                    profileId: profileId,
                    ownerId: ownerId,
                    sessionRevision: sessionRevision
                )
                apply(result)
                observations = try await store.fetchObservations(
                    profileId: profileId, ownerId: ownerId)
            } else {
                try await coordinator.stopObservation()
                updateStatusText()
            }
        } catch {
            currentStatusText = "Error updating settings"
        }
    }

    private func updateStatusText() {
        if !isOptedIn {
            currentStatusText = "Disabled (P056 discovery foundation)"
        } else if pauseDiscovery {
            currentStatusText = "Discovery paused"
        } else if !permissionState.isAuthorized {
            currentStatusText = "Permission required (\(permissionState.rawValue))"
        } else {
            currentStatusText = "Discovery ready"
        }
    }

    private func apply(_ result: PhotoDiscoveryScanResult) {
        switch result {
        case .completed(let count, _):
            currentStatusText = "Discovery active (\(count) assets observed)"
        case .disabled:
            currentStatusText = "Discovery disabled"
        case .permissionDenied(let state):
            currentStatusText = "Permission required (\(state.rawValue))"
        case .interrupted(let reason):
            currentStatusText = "Interrupted: \(reason)"
        case .partial(let observed, let total, _):
            currentStatusText = "Partial discovery (\(observed)/\(total)); inventory is incomplete"
        }
    }

    public func resourceLabel(_ resource: PhotoResourceSelection) -> String {
        let role: String
        switch resource.role {
        case .primaryImage: role = "Still image"
        case .primaryVideo: role = "Video"
        case .motionVideo: role = "Live Photo motion video"
        case .auxiliary: role = "Auxiliary resource"
        }
        let rendition: String
        switch resource.rendition {
        case .original: rendition = "Original"
        case .edited: rendition = "Edited rendition"
        case .pairedMotion: rendition = "Paired original motion"
        case .auxiliary: rendition = "Auxiliary"
        }
        return "\(rendition) · \(role) · \(resource.uniformTypeIdentifier ?? "unknown type")"
    }

    private func label(for state: PhotoResourceExtractionState) -> String {
        switch state {
        case .discovered: "Discovered"
        case .eligible: "Ready to prepare"
        case .extracting: "Extracting original resource…"
        case .verifying: "Verifying length and SHA-256…"
        case .verifiedReady: "Verified and ready for foreground upload"
        case .handoffPending: "Waiting for explicit upload handoff"
        case .handedOff: "Handed to the P045 foreground uploader"
        case .uploadedConfirmed: "Uploaded and confirmed by the server"
        case .sourceUnavailable: "Original is not available locally; iCloud download is off"
        case .permissionRevoked: "Photos permission or account session changed"
        case .cancelled: "Preparation cancelled"
        case .failedRecoverable: "Preparation failed; retry is available"
        case .failedTerminal: "Resource verification failed"
        }
    }

    private func message(for failure: PhotoResourceTransferFailure) -> String {
        switch failure {
        case .permissionDenied: "Photos access is required. No upload was started."
        case .sourceUnavailable:
            "The original is not stored locally. iCloud download is disabled, and no preview was substituted."
        case .sourceChanged: "The selected resource changed during preparation."
        case .unsupportedResource: "This PhotoKit resource type is not supported."
        case .resourceTooLarge: "This resource exceeds the local extraction size limit."
        case .emptyResource: "PhotoKit returned an empty original resource."
        case .insufficientStorage: "There is not enough free storage to prepare this resource."
        case .stagingQuotaExceeded: "The shared private transfer staging quota is full."
        case .integrityMismatch: "The staged byte length or SHA-256 did not match."
        case .staleAuthorization: "The signed-in account, device, or permission session changed."
        case .ineligibleObservation: "This asset is no longer eligible for preparation."
        case .configurationDisabled: "Photo discovery or video inclusion is currently disabled."
        case .extractionAlreadyInProgress:
            "Another original resource is being prepared. Wait for it to finish first."
        case .existingUnresolvedOperation:
            "This resource already has an unresolved operation. Resume it before preparing another copy."
        case .cancelled: "Preparation was cancelled."
        case .storageFailure: "Private staging or durable state could not be saved."
        }
    }

    private func updateUploadStatus(_ result: FileUploadResult) {
        switch result {
        case .completed(let record):
            uploadStatusText = record.state == .committed
                ? "Uploaded and confirmed by the server." : Self.uploadLabel(for: record.state)
        case .scheduled(let record):
            uploadStatusText = Self.uploadLabel(for: record.state)
        case .stopped(let record, let failure):
            uploadStatusText = record.map { Self.uploadLabel(for: $0.state) }
                ?? "Foreground upload did not start."
            failureReasonText = String(describing: failure)
        }
    }

    private static func uploadLabel(for state: UploadLocalState) -> String {
        switch state {
        case .committed: "Uploaded and confirmed by the server."
        case .sessionCreating, .completing, .outcomeUnknown, .recoveryRequired:
            "Awaiting server confirmation or reconciliation."
        case .sessionOpen, .uploading, .readyToComplete:
            "Foreground upload in progress."
        case .cancelledLocal, .failedRecoverable, .sourceStaged:
            "Foreground upload is paused or ready to resume."
        default: "Foreground upload stopped."
        }
    }
}
