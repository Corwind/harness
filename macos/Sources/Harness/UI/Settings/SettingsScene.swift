import SwiftUI

public struct SettingsScene: Scene {
    @Bindable private var viewModel: SettingsViewModel

    public init(viewModel: SettingsViewModel) {
        self.viewModel = viewModel
    }

    public var body: some Scene {
        SwiftUI.Settings {
            SettingsRoot(viewModel: viewModel)
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

    var body: some View {
        Group {
            if let viewModel {
                SettingsRoot(viewModel: viewModel)
                    .theme(viewModel.currentTheme)
                    .task {
                        await viewModel.load()
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
                viewModel = Self.makeViewModel(session: session)
            }
        }
        .onAppear {
            if viewModel == nil, let session = state.session {
                viewModel = Self.makeViewModel(session: session)
            }
        }
    }

    private static func makeViewModel(session: BackendSession) -> SettingsViewModel {
        let client = HTTPClient(baseURL: session.baseURL, token: session.token)
        return SettingsViewModel(
            settings: SettingsGatewayAdapter(client: client),
            providers: ProvidersGatewayAdapter(client: client)
        )
    }
}

struct SettingsRoot: View {
    @Bindable var viewModel: SettingsViewModel

    var body: some View {
        TabView {
            GeneralTab(viewModel: viewModel)
                .tabItem { Label("General", systemImage: "gearshape") }
            ProvidersTab(viewModel: viewModel)
                .tabItem { Label("Providers", systemImage: "cloud") }
            ModelsTab(viewModel: viewModel)
                .tabItem { Label("Models", systemImage: "brain") }
        }
        .background(viewModel.currentTheme.background)
    }
}
