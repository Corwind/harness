import SwiftUI

// `@main` lives in this file, which is excluded from the SwiftPM target so
// the test runner's `main` symbol does not collide with it. The Xcode .app
// target compiles this file; SPM builds (CI, agent work) skip it.
@main
struct HarnessAppMain: App {
    var body: some Scene {
        HarnessApp().body
    }
}
