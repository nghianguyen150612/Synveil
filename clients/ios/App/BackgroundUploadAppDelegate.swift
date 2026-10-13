import UIKit

@MainActor
final class BackgroundUploadAppDelegate: NSObject, UIApplicationDelegate {
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
    }
}
