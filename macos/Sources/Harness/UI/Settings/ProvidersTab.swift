import SwiftUI

struct ProvidersTab: View {
    @Bindable var viewModel: SettingsViewModel
    @State private var selectedProviderId: String?
    @State private var apiKeyDraft: String = ""
    @State private var baseUrlDraft: String = ""

    var body: some View {
        HSplitView {
            providerList
                .frame(minWidth: 180)
            providerDetail
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

    private var providerList: some View {
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
