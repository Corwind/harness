import SwiftUI

struct ModelsTab: View {
    @Bindable var viewModel: SettingsViewModel
    @State private var selectedProviderId: String?

    var body: some View {
        HSplitView {
            providerSidebar
                .frame(minWidth: 180)
            modelsList
                .frame(minWidth: 360)
        }
        .background(viewModel.currentTheme.background)
        .foregroundStyle(viewModel.currentTheme.text)
        .task {
            if viewModel.providersList.isEmpty {
                await viewModel.refreshProviders()
            }
        }
    }

    private var providerSidebar: some View {
        List(selection: $selectedProviderId) {
            Section("Configured providers") {
                ForEach(viewModel.providersList.filter(\.configured), id: \.id) { provider in
                    Text(provider.displayName)
                        .foregroundStyle(viewModel.currentTheme.text)
                        .tag(Optional(provider.id))
                }
            }
        }
        .listStyle(.sidebar)
        .onChange(of: selectedProviderId) { _, newValue in
            guard let id = newValue else { return }
            Task { await viewModel.refreshModels(providerId: id) }
        }
    }

    @ViewBuilder
    private var modelsList: some View {
        if let id = selectedProviderId {
            VStack(alignment: .leading) {
                HStack {
                    Text("Models")
                        .font(.title2)
                        .foregroundStyle(viewModel.currentTheme.text)
                    Spacer()
                    Button("Refresh") {
                        Task { await viewModel.refreshModels(providerId: id) }
                    }
                    .disabled(viewModel.isLoadingModels.contains(id))
                }
                if let message = viewModel.modelsLoadError[id] {
                    Text(message)
                        .font(.callout)
                        .foregroundStyle(viewModel.currentTheme.error)
                }
                List(viewModel.models[id] ?? [], id: \.id) { model in
                    VStack(alignment: .leading) {
                        Text(model.displayName)
                            .foregroundStyle(viewModel.currentTheme.text)
                        Text(model.id)
                            .font(.caption)
                            .foregroundStyle(viewModel.currentTheme.mutedText)
                    }
                }
            }
            .padding()
        } else {
            VStack {
                Spacer()
                Text("Select a configured provider to list its models.")
                    .foregroundStyle(viewModel.currentTheme.mutedText)
                Spacer()
            }
            .frame(maxWidth: .infinity)
        }
    }
}
