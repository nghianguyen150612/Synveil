import XCTest
import SwiftUI
@testable import Synveil

final class SynveilBootstrapTests: XCTestCase {
    func testBootstrapViewCanBeInstantiated() {
        let view = BootstrapView()
        XCTAssertNotNil(view.body)
    }
}
