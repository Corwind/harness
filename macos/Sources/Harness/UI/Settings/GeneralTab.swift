import SwiftUI

struct GeneralTab: View {
    @Bindable var viewModel: SettingsViewModel

    var body: some View {
        Form {
            Section("Appearance") {
                Picker("Theme", selection: themeBinding) {
                    Text("Light").tag(SettingsTheme.light)
                    Text("Dark").tag(SettingsTheme.dark)
                    Text("System").tag(SettingsTheme.system)
                }
                .pickerStyle(.segmented)
            }
        }
        .padding()
        .background(viewModel.currentTheme.background)
        .foregroundStyle(viewModel.currentTheme.text)
    }

    private var themeBinding: Binding<SettingsTheme> {
        Binding(
            get: { viewModel.appTheme },
            set: { newValue in
                Task { await viewModel.setTheme(newValue) }
            }
        )
    }
}
