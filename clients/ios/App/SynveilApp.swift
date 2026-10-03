import SwiftUI

@main
struct SynveilApp: App {
    private let configuration: AppConfiguration

    init() {
        self.configuration = AppConfiguration.load()
    }

    var body: some Scene {
        WindowGroup {
            BootstrapView()
        }
    }
}
