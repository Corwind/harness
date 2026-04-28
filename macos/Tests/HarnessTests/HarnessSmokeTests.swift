import XCTest
@testable import HarnessApp

final class HarnessSmokeTests: XCTestCase {
    func testPackageBuildsAndImports() {
        // Behavior: the package builds and the bootstrap root view is constructible.
        // Real behavior tests land per-feature in Phase 1.
        _ = HarnessRoot()
    }
}
