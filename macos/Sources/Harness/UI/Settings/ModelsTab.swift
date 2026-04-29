import SwiftUI

struct ModelsTab: View {
    @Bindable var viewModel: SettingsViewModel
    @State private var selectedProviderId: String?

    var body: some View {
        VStack(spacing: 0) {
            if let banner = viewModel.providersBannerError {
                BannerView(
                    message: banner.userMessage,
                    theme: viewModel.currentTheme,
                    isLoading: viewModel.isLoadingProviders,
                    retry: { Task { await viewModel.refreshProviders() } }
                )
            }
            HSplitView {
                providerSidebar
                    .frame(minWidth: 200)
                modelsList
                    .frame(minWidth: 360)
            }
        }
        .background(viewModel.currentTheme.background)
        .foregroundStyle(viewModel.currentTheme.text)
        .task {
            if !viewModel.hasLoadedProviders {
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
            modelsDetail(for: id)
        } else if viewModel.providersList.filter(\.configured).isEmpty
                    && viewModel.hasLoadedProviders {
            VStack(spacing: 8) {
                Spacer()
                Text("No configured providers yet.")
                    .foregroundStyle(viewModel.currentTheme.text)
                Text("Add an API key in the Providers tab to load models.")
                    .font(.caption)
                    .foregroundStyle(viewModel.currentTheme.mutedText)
                Spacer()
            }
            .frame(maxWidth: .infinity)
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

    private func modelsDetail(for id: String) -> some View {
        let isLoading = viewModel.isLoadingModels.contains(id)
        let typedError = viewModel.modelsError(for: id)
        let hasResults = !(viewModel.models[id] ?? []).isEmpty
        let isUnconfigured = typedError == .unconfigured

        return VStack(alignment: .leading) {
            HStack {
                Text("Models")
                    .font(.title2)
                    .foregroundStyle(viewModel.currentTheme.text)
                Spacer()
                Button("Refresh") {
                    Task { await viewModel.refreshModels(providerId: id) }
                }
                .disabled(isLoading || isUnconfigured)
            }
            if isUnconfigured {
                Text(viewModel.modelsErrorMessage(for: id))
                    .font(.callout)
                    .foregroundStyle(viewModel.currentTheme.mutedText)
            } else if let typedError {
                modelsErrorBanner(for: id, error: typedError)
            }
            if isLoading && !hasResults {
                VStack(spacing: 8) {
                    Spacer()
                    ProgressView()
                    Text("Loading models…")
                        .font(.caption)
                        .foregroundStyle(viewModel.currentTheme.mutedText)
                    Spacer()
                }
                .frame(maxWidth: .infinity)
            } else if !hasResults && typedError == nil {
                VStack {
                    Spacer()
                    Text("No models returned by provider")
                        .foregroundStyle(viewModel.currentTheme.mutedText)
                    Spacer()
                }
                .frame(maxWidth: .infinity)
            } else {
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
        }
        .padding()
    }

    @ViewBuilder
    private func modelsErrorBanner(for id: String, error: ProviderConfigError) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(viewModel.currentTheme.error)
            Text(viewModel.modelsErrorMessage(for: id))
                .font(.callout)
                .foregroundStyle(viewModel.currentTheme.text)
            Spacer()
            Button("Retry") {
                Task { await viewModel.refreshModels(providerId: id) }
            }
            .buttonStyle(.borderless)
            .disabled(viewModel.isLoadingModels.contains(id))
        }
        .padding(8)
        .background(viewModel.currentTheme.error.opacity(0.08))
    }
}
