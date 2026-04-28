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
