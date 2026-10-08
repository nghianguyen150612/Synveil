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
                credentialSink: container.credentialSink
            )
                .task {
                    await container.prepareEnrollmentSecurity()
                    await container.sessionController.start()
                }
        }
    }
}
