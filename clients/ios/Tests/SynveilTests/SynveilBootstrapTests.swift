import SwiftUI
import XCTest

@testable import Synveil

final class SynveilBootstrapTests: XCTestCase {
    func testBootstrapViewCanBeInstantiated() {
        let view = BootstrapView()
        XCTAssertNotNil(view.body)
    }
}
