import SwiftUI

@main
struct SynveilApp: App {
    @UIApplicationDelegateAdaptor(BackgroundUploadAppDelegate.self) private var appDelegate
    @State private var container: AppDependencyContainer

    init() {
        let configuration = AppConfiguration.load()
        let dependencies = AppDependencyContainer(configuration: configuration)
        _container = State(initialValue: dependencies)
        BGAppRefreshScheduler.shared.install(
            operation: { await dependencies.runBackgroundMetadata($0) },
            identity: { try await dependencies.backgroundSettingsIdentity() },
            savedConfirmation: { try await dependencies.backgroundSavedConfirmation() },
            optInIdentity: { try await dependencies.authorizeBackgroundOptIn() },
            recoveryInspection: { try await dependencies.backgroundRecoveryRequired() })
        BackgroundUploadApplicationBootstrap.shared.install {
            await dependencies.prepareApplicationSession()
            await dependencies.restoreBackgroundTransfers()
            BackgroundUploadEventDelivery.shared.processPendingEvents()
            await BGAppRefreshScheduler.shared.schedule()
        }
    }

    var body: some Scene {
        WindowGroup {
            RootView(
                sessionController: container.sessionController,
                rustBridge: container.rustBridge,
                credentialSink: container.credentialSink,
                libraryCatalog: container.libraryCatalog,
                nodeRepository: container.nodeRepository,
                offlineNodeBrowserService: container.offlineNodeBrowserService,
                savedMetadataInspection: container.savedMetadataInspection,
                metadataMutationFeature: container.metadataMutationFeature,
                inboundSyncCoordinator: container.inboundSyncCoordinator,
                nodeProjectionRepository: container.nodeProjectionRepository,
                rebaselineCoordinator: container.rebaselineCoordinator,
                syncCheckpointService: container.syncCheckpointService,
                downloadCoordinator: container.downloadCoordinator,
                uploadCoordinator: container.uploadCoordinator,
                fileExportCoordinator: container.fileExportCoordinator
            )
            .task {
                await BackgroundUploadApplicationBootstrap.shared.waitForStartup()
            }
        }
    }
}
