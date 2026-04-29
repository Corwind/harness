import SwiftUI

struct ProvidersTab: View {
    @Bindable var viewModel: SettingsViewModel
    @State private var selectedProviderId: String?
    @State private var apiKeyDraft: String = ""
    @State private var baseUrlDraft: String = ""

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
                providerList
                    .frame(minWidth: 200)
                providerDetail
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

    @ViewBuilder
    private var providerList: some View {
        if viewModel.shouldShowAddProviderEmptyState {
            emptyProviderCTA
        } else if viewModel.isLoadingProviders && viewModel.providersList.isEmpty {
            loadingSkeleton
        } else {
            populatedList
        }
    }

    private var populatedList: some View {
        List(selection: $selectedProviderId) {
            Section("Providers") {
                ForEach(viewModel.providersList, id: \.id) { provider in
                    HStack {
                        VStack(alignment: .leading) {
                            Text(provider.displayName)
                                .foregroundStyle(viewModel.currentTheme.text)
                            Text(provider.id)
                                .font(.caption)
                                .foregroundStyle(viewModel.currentTheme.mutedText)
                        }
                        Spacer()
                        if provider.configured {
                            Text("Configured")
                                .font(.caption)
                                .foregroundStyle(viewModel.currentTheme.success)
                        }
                    }
                    .tag(Optional(provider.id))
                }
            }
        }
        .listStyle(.sidebar)
    }

    private var loadingSkeleton: some View {
        VStack(spacing: 8) {
            Spacer()
            ProgressView()
            Text("Loading providers…")
                .font(.caption)
                .foregroundStyle(viewModel.currentTheme.mutedText)
            Spacer()
        }
        .frame(maxWidth: .infinity)
    }

    private var emptyProviderCTA: some View {
        VStack(spacing: 12) {
            Spacer()
            Image(systemName: "key")
                .font(.system(size: 36))
                .foregroundStyle(viewModel.currentTheme.mutedText)
            Text("Add an API key to get started")
                .font(.headline)
                .foregroundStyle(viewModel.currentTheme.text)
            Text("Provider list is empty. Once you configure a provider it'll appear here.")
                .font(.caption)
                .foregroundStyle(viewModel.currentTheme.mutedText)
                .multilineTextAlignment(.center)
                .padding(.horizontal)
            Button {
                Task { await viewModel.refreshProviders() }
            } label: {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .buttonStyle(.bordered)
            Spacer()
        }
        .frame(maxWidth: .infinity)
    }

    @ViewBuilder
    private var providerDetail: some View {
        if let id = selectedProviderId,
           let provider = viewModel.providersList.first(where: { $0.id == id }) {
            VStack(alignment: .leading, spacing: 12) {
                Text(provider.displayName)
                    .font(.title2)
                    .foregroundStyle(viewModel.currentTheme.text)
                Form {
                    TextField("API key", text: $apiKeyDraft)
                        .textFieldStyle(.roundedBorder)
                    TextField("Base URL (optional)", text: $baseUrlDraft)
                        .textFieldStyle(.roundedBorder)
                }
                if let error = viewModel.providerError {
                    Text(error.userMessage)
                        .foregroundStyle(viewModel.currentTheme.error)
                        .font(.callout)
                }
                HStack {
                    Button("Save") {
                        Task {
                            await viewModel.upsertProvider(
                                id: provider.id,
                                apiKey: apiKeyDraft,
                                baseUrl: baseUrlDraft.isEmpty ? nil : baseUrlDraft
                            )
                        }
                    }
                    .disabled(viewModel.isSavingProvider)
                    if viewModel.isSavingProvider {
                        ProgressView().controlSize(.small)
                    }
                }
                Spacer()
            }
            .padding()
        } else if viewModel.shouldShowAddProviderEmptyState {
            VStack {
                Spacer()
                Text("Configure a provider on the left to start chatting.")
                    .foregroundStyle(viewModel.currentTheme.mutedText)
                Spacer()
            }
            .frame(maxWidth: .infinity)
        } else {
            VStack {
                Spacer()
                Text("Select a provider to configure its API key.")
                    .foregroundStyle(viewModel.currentTheme.mutedText)
                Spacer()
            }
            .frame(maxWidth: .infinity)
        }
    }
}

/// Shared retryable banner used at the top of Settings tabs when the
/// underlying gateway call failed.
struct BannerView: View {
    let message: String
    let theme: Theme
    let isLoading: Bool
    let retry: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(theme.error)
            Text(message)
                .font(.callout)
                .foregroundStyle(theme.text)
            Spacer()
            if isLoading {
                ProgressView().controlSize(.small)
            } else {
                Button("Retry", action: retry)
                    .buttonStyle(.borderless)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(theme.error.opacity(0.08))
    }
}
