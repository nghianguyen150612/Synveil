import SwiftUI

@main
struct SynveilApp: App {
    @State private var container: AppDependencyContainer

    init() {
        let configuration = AppConfiguration.load()
        _container = State(initialValue: AppDependencyContainer(configuration: configuration))
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
                metadataMutationFeature: container.metadataMutationFeature,
                inboundSyncCoordinator: container.inboundSyncCoordinator,
                nodeProjectionRepository: container.nodeProjectionRepository,
                rebaselineCoordinator: container.rebaselineCoordinator,
                syncCheckpointService: container.syncCheckpointService
            )
            .task {
                await container.prepareEnrollmentSecurity()
                guard !Task.isCancelled else { return }
                await container.sessionController.start()
            }
        }
    }
}
