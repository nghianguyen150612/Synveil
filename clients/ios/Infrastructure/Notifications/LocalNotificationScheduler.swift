import Foundation
import UserNotifications

@MainActor
final class LocalNotificationScheduler: LocalNotificationScheduling {
    private let center: UNUserNotificationCenter
    init(center: UNUserNotificationCenter = .current()) { self.center = center }
    func authorization() async -> NotificationAuthorization {
        let settings = await center.notificationSettings()
        switch settings.authorizationStatus {
        case .authorized:
            return settings.alertSetting == .enabled ? .authorized : .unavailable
        case .notDetermined: return .notDetermined
        case .denied: return .denied
        case .provisional: return .provisional
        case .ephemeral: return .ephemeral
        @unknown default: return .unavailable
        }
    }
    func requestAuthorization() async throws {
        _ = try await center.requestAuthorization(options: [.alert])
    }
    func pendingIdentifiers() async -> [String] {
        await center.pendingNotificationRequests().map(\.identifier)
    }
    func schedule(_ payload: LocalNotificationPayload) async throws {
        let content = UNMutableNotificationContent()
        content.title = payload.title
        content.body = payload.body
        content.userInfo = payload.userInfo
        // No sound, badge, critical alert, time-sensitive interruption or remote registration.
        try await center.add(
            UNNotificationRequest(identifier: payload.identifier, content: content, trigger: nil))
    }
    func remove(_ identifiers: [String]) {
        let owned = identifiers.filter { $0.hasPrefix(NotificationLimits.prefix) }
        center.removePendingNotificationRequests(withIdentifiers: owned)
        center.removeDeliveredNotifications(withIdentifiers: owned)
    }
}
