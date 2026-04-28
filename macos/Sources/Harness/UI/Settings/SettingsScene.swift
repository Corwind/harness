import SwiftUI

public struct SettingsScene: Scene {
    @Bindable private var viewModel: SettingsViewModel
    private let sandboxes: SandboxTemplatesViewModel?

    public init(viewModel: SettingsViewModel, sandboxes: SandboxTemplatesViewModel? = nil) {
        self.viewModel = viewModel
        self.sandboxes = sandboxes
    }

    public var body: some Scene {
        SwiftUI.Settings {
            SettingsRoot(viewModel: viewModel, sandboxes: sandboxes)
                .frame(minWidth: 720, minHeight: 480)
                .theme(viewModel.currentTheme)
                .task {
                    await viewModel.load()
                }
        }
    }
}

/// Scene wrapper used by the App composition root. Always present (so the
/// macOS Settings menu item is available from launch), but only builds the
/// gateways and view model once a `BackendSession` has been acquired.
struct SettingsHostScene: Scene {
    let state: BootstrapState

    var body: some Scene {
        SwiftUI.Settings {
            SettingsHost(state: state)
                .frame(minWidth: 720, minHeight: 480)
        }
    }
}

private struct SettingsHost: View {
    let state: BootstrapState
    @State private var viewModel: SettingsViewModel?
    @State private var sandboxes: SandboxTemplatesViewModel?

    var body: some View {
        Group {
            if let viewModel {
                SettingsRoot(viewModel: viewModel, sandboxes: sandboxes)
                    .theme(viewModel.currentTheme)
                    .task {
                        await viewModel.load()
                        await sandboxes?.load()
                    }
            } else {
                VStack(spacing: 12) {
                    ProgressView()
                    Text("Waiting for backend…")
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .onChange(of: state.session) { _, newSession in
            if viewModel == nil, let session = newSession {
                instantiate(session: session)
            }
        }
        .onAppear {
            if viewModel == nil, let session = state.session {
                instantiate(session: session)
            }
        }
    }

    private func instantiate(session: BackendSession) {
        let client = HTTPClient(baseURL: session.baseURL, token: session.token)
        viewModel = SettingsViewModel(
            settings: SettingsGatewayAdapter(client: client),
            providers: ProvidersGatewayAdapter(client: client)
        )
        sandboxes = SandboxTemplatesViewModel(
            gateway: SandboxTemplatesGatewayAdapter(client: client)
        )
    }
}

struct SettingsRoot: View {
    @Bindable var viewModel: SettingsViewModel
    let sandboxes: SandboxTemplatesViewModel?

    var body: some View {
        TabView {
            GeneralTab(viewModel: viewModel)
                .tabItem { Label("General", systemImage: "gearshape") }
            ProvidersTab(viewModel: viewModel)
                .tabItem { Label("Providers", systemImage: "cloud") }
            ModelsTab(viewModel: viewModel)
                .tabItem { Label("Models", systemImage: "brain") }
            if let sandboxes {
                SandboxesTab(viewModel: sandboxes)
                    .theme(viewModel.currentTheme)
                    .tabItem { Label("Sandboxes", systemImage: "shield.lefthalf.filled") }
            }
        }
        .background(viewModel.currentTheme.background)
    }
}
