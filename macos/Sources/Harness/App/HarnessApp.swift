import Foundation
import SwiftUI

public struct HarnessApp: App {
    @State private var state: BootstrapState = .loading

    public init() {}

    public var body: some Scene {
        WindowGroup("Harness") {
            HarnessRoot(state: state)
                .task {
                    await bootstrap()
                }
        }
        SettingsHostScene(state: state)
    }

    private func bootstrap() async {
        let provider: BackendSessionProvider
        do {
            provider = try Self.resolveProvider()
        } catch {
            state = .failed(error)
            return
        }
        do {
            let session = try await provider.acquire()
            state = .ready(session)
        } catch {
            state = .failed(error)
        }
    }

    static func resolveProvider() throws -> BackendSessionProvider {
        if let envPath = ProcessInfo.processInfo.environment["HARNESS_BACKEND_PATH"],
           !envPath.isEmpty {
            return SidecarLauncher(executableURL: URL(fileURLWithPath: envPath))
        }
        if let bundled = Bundle.main.url(forResource: "harness-server", withExtension: nil) {
            return SidecarLauncher(executableURL: bundled)
        }
        throw BackendSessionError.backendNotFound(path: "<HARNESS_BACKEND_PATH unset and no bundled binary>")
    }
}

enum BootstrapState {
    case loading
    case ready(BackendSession)
    case failed(Error)

    var session: BackendSession? {
        if case .ready(let s) = self { return s }
        return nil
    }
}

public struct HarnessRoot: View {
    let state: BootstrapState

    init(state: BootstrapState = .loading) {
        self.state = state
    }

    public init() {
        self.state = .loading
    }

    public var body: some View {
        switch state {
        case .loading:
            VStack(spacing: 12) {
                ProgressView()
                Text("Starting backend…")
                    .foregroundStyle(.secondary)
            }
            .frame(minWidth: 480, minHeight: 320)
        case .ready(let session):
            RootView(session: session)
        case .failed(let error):
            VStack(spacing: 8) {
                Text("Backend failed to start")
                    .font(.headline)
                    .foregroundStyle(.red)
                Text(String(describing: error))
                    .font(.system(.caption, design: .monospaced))
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
            }
            .frame(minWidth: 480, minHeight: 320)
            .padding()
        }
    }
}
