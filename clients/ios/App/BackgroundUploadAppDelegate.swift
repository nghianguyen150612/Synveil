import UIKit
import UserNotifications

@MainActor
final class BackgroundUploadAppDelegate: NSObject, UIApplicationDelegate,
    UNUserNotificationCenterDelegate
{
    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        BGAppRefreshScheduler.shared.register()
        BackgroundUploadApplicationBootstrap.shared.start()
        return true
    }

    func applicationDidEnterBackground(_ application: UIApplication) {
        AppLifecycleEventDelivery.shared.receive(.background)
        Task { await BGAppRefreshScheduler.shared.schedule() }
    }

    func applicationWillResignActive(_ application: UIApplication) {
        AppLifecycleEventDelivery.shared.receive(.inactive)
    }

    func applicationDidBecomeActive(_ application: UIApplication) {
        AppLifecycleEventDelivery.shared.receive(.active)
        Task { await BGAppRefreshScheduler.shared.refreshStatus() }
        NotificationApplicationDelivery.shared.becameActive?()
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler completionHandler:
            @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        // Foreground outcomes remain on their canonical screen. No global modal or duplicate
        // banner; the independent observable presentation counter does not claim the user saw it.
        completionHandler([])
        Task { @MainActor in NotificationApplicationDelivery.shared.foreground?() }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let request = response.notification.request
        let info = request.content.userInfo as? [String: String] ?? [:]
        let handle = NotificationLimits.handle(identifier: request.identifier, userInfo: info)
        let selected = response.actionIdentifier == UNNotificationDefaultActionIdentifier
        completionHandler()
        guard selected, let handle else { return }
        Task { @MainActor in NotificationApplicationDelivery.shared.receive(handle) }
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

enum AppLifecyclePhase: Equatable, Sendable {
    case active, inactive, background
}

/// Relays equivalent UIKit and SwiftUI app-level lifecycle events to one owner.
@MainActor
final class AppLifecycleEventDelivery {
    static let shared = AppLifecycleEventDelivery()
    private var handler: (@MainActor (AppLifecyclePhase) -> Void)?
    private var currentPhase: AppLifecyclePhase?

    func install(_ handler: @escaping @MainActor (AppLifecyclePhase) -> Void) {
        guard self.handler == nil else { return }
        self.handler = handler
        if let currentPhase { handler(currentPhase) }
    }

    func receive(_ phase: AppLifecyclePhase) {
        currentPhase = phase
        handler?(phase)
    }
}

/// Coalesces duplicate lifecycle notifications and advances one callback fence for every phase
/// transition. Transfer owners cancel only when the app is not active. It owns no transfer state
/// or persistence.
@MainActor
final class AppLifecycleRecoveryCoordinator {
    private(set) var phase: AppLifecyclePhase?
    private(set) var generation: UInt64 = 0

    func receive(
        _ phase: AppLifecyclePhase,
        lifecycleChanged: @MainActor (UInt64, Bool) -> Void
    ) {
        guard self.phase != phase else { return }
        self.phase = phase
        generation &+= 1
        lifecycleChanged(generation, phase == .active)
    }
}

/// One bounded opaque response slot covers callbacks before dependency installation. Native
/// completion handlers never wait for session restoration, storage, network reads or a scene.
@MainActor
final class NotificationApplicationDelivery {
    static let shared = NotificationApplicationDelivery()
    var becameActive: (@MainActor () -> Void)?
    var foreground: (@MainActor () -> Void)?
    private var handler: (@MainActor (UUID) -> Void)?
    private var pending: (UUID, Date)?
    func install(_ handler: @escaping @MainActor (UUID) -> Void) {
        guard self.handler == nil else { return }
        self.handler = handler
        if let pending, Date().timeIntervalSince(pending.1) < NotificationLimits.intentLifetime {
            handler(pending.0)
        }
        pending = nil
    }
    func receive(_ id: UUID) {
        if let handler { handler(id) } else { pending = (id, Date()) }
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
