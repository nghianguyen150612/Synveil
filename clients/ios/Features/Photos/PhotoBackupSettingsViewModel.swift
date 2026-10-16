import Foundation
import Observation

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

    private let permissionAdapter: PhotoLibraryPermissionChecking
    private let coordinator: PhotoLibraryObservationCoordinatorProtocol
    private let store: PhotoObservationStoreProtocol
    private let profileId: UUID
    private let ownerId: String

    public init(
        profileId: UUID,
        ownerId: String,
        permissionAdapter: PhotoLibraryPermissionChecking = PhotoLibraryPermissionAdapter(),
        coordinator: PhotoLibraryObservationCoordinatorProtocol,
        store: PhotoObservationStoreProtocol
    ) {
        self.profileId = profileId
        self.ownerId = ownerId
        self.permissionAdapter = permissionAdapter
        self.coordinator = coordinator
        self.store = store
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
            updateStatusText()
        } catch {
            currentStatusText = "Error loading configuration"
        }
    }

    public func requestPermission() async {
        permissionState = await permissionAdapter.requestPermission()
        updateStatusText()
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
                let result = try await coordinator.startObservation(profileId: profileId, ownerId: ownerId)
                switch result {
                case .completed(let count, _):
                    currentStatusText = "Discovery active (\(count) assets discovered)"
                case .disabled:
                    currentStatusText = "Discovery disabled"
                case .permissionDenied(let state):
                    currentStatusText = "Permission required (\(state.rawValue))"
                case .interrupted(let reason):
                    currentStatusText = "Interrupted: \(reason)"
                case .partial(let observed, let total, _):
                    currentStatusText = "Partial discovery (\(observed)/\(total))"
                }
            } else {
                await coordinator.stopObservation()
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
}
