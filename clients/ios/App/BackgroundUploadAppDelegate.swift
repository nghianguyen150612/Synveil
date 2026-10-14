import UIKit

@MainActor
final class BackgroundUploadAppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        BGAppRefreshScheduler.shared.register()
        BackgroundUploadApplicationBootstrap.shared.start()
        return true
    }

    func applicationDidEnterBackground(_ application: UIApplication) {
        Task { await BGAppRefreshScheduler.shared.schedule() }
    }

    func applicationDidBecomeActive(_ application: UIApplication) {
        Task { await BGAppRefreshScheduler.shared.refreshStatus() }
    }

    func application(
        _ application: UIApplication,
        handleEventsForBackgroundURLSession identifier: String,
        completionHandler: @escaping () -> Void
    ) {
        guard identifier == URLSessionBackgroundUploadTransport.identifier else {
            completionHandler()
            return
        }
        BackgroundUploadEventDelivery.shared.installCompletion(completionHandler)
        BackgroundUploadApplicationBootstrap.shared.start()
    }
}

/// Bootstrap is owned by the application lifecycle, including launches without a connected scene.
/// SwiftUI can await the same task but cannot create a second storage/session/recovery pass.
@MainActor
final class BackgroundUploadApplicationBootstrap {
    static let shared = BackgroundUploadApplicationBootstrap()
    private var operation: (@MainActor () async -> Void)?
    private var task: Task<Void, Never>?
    private var requested = false

    func install(_ operation: @escaping @MainActor () async -> Void) {
        guard self.operation == nil else { return }
        self.operation = operation
        if requested { start() }
    }

    func start() {
        requested = true
        guard task == nil, let operation else { return }
        task = Task { @MainActor in await operation() }
    }

    func waitForStartup() async {
        start()
        await task?.value
    }
}
