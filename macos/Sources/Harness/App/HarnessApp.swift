import SwiftUI

// Entry point. Filled in by Phase 1 / track F.
// The real entry point will spawn the Rust sidecar from Bundle.main, read
// `{port, token}` from its stdout, and inject a `BackendSession` into the
// SwiftUI environment.

public struct HarnessRoot: View {
    public init() {}
    public var body: some View {
        Text("Harness — bootstrap")
            .padding()
    }
}
