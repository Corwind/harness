import XCTest
@testable import HarnessApp

final class HarnessSmokeTests: XCTestCase {
    func testPackageBuildsAndRootViewIsConstructible() {
        // Behavior: the package builds and the bootstrap root view is constructible.
        _ = HarnessRoot()
    }
}
