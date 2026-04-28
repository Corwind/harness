import SwiftUI

/// The window root once the backend session has been acquired. Builds the
/// HTTP client + gateways, hands them to a `ConversationTabsViewModel`,
/// and renders a NavigationSplitView with a sidebar of conversations and a
/// detail pane hosting the active chat.
struct RootView: View {
    let session: BackendSession
    @State private var tabsVM: ConversationTabsViewModel
    @State private var providers: [Provider] = []
    @State private var models: [String: [Model]] = [:]
    @State private var providersError: ChatError? = nil
    @State private var didLoad: Bool = false

    private let providersGateway: ProvidersGateway

    init(session: BackendSession) {
        self.session = session
        let client = HTTPClient(baseURL: session.baseURL, token: session.token)
        let runGateway = RunGatewayAdapter(client: client)
        let messageGateway = MessageGatewayAdapter(client: client)
        let conversationGateway = ConversationGatewayAdapter(client: client)
        let providersGateway = ProvidersGatewayAdapter(client: client)
        let sandboxGateway = SandboxTemplatesGatewayAdapter(client: client)
        self.providersGateway = providersGateway
        _tabsVM = State(initialValue: ConversationTabsViewModel(
            conversationGateway: conversationGateway,
            messageGateway: messageGateway,
            runGateway: runGateway,
            providersGateway: providersGateway,
            sandboxGateway: sandboxGateway
        ))
    }

    var body: some View {
        NavigationSplitView {
            ConversationSidebarView(
                viewModel: tabsVM,
                providers: providers,
                models: models
            )
            .frame(minWidth: 240)
        } detail: {
            detailPane
        }
        .task {
            if !didLoad {
                didLoad = true
                await tabsVM.load()
                await loadProvidersAndModels()
            }
        }
    }

    @ViewBuilder
    private var detailPane: some View {
        if let activeId = tabsVM.activeConversationId {
            ChatView(viewModel: tabsVM.chatViewModel(for: activeId))
                .id(activeId)
        } else if tabsVM.isLoading {
            VStack(spacing: 12) {
                ProgressView()
                Text("Loading conversations…")
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            EmptyStatePane(
                error: tabsVM.error ?? providersError,
                hasProviders: !providers.isEmpty
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func loadProvidersAndModels() async {
        do {
            let list = try await providersGateway.list()
            self.providers = list
            for provider in list where provider.configured {
                do {
                    let modelList = try await providersGateway.listModels(providerId: provider.id)
                    self.models[provider.id] = modelList
                } catch {
                    // Models for one provider failing must not block others;
                    // user will see an empty model picker for that provider.
                    self.models[provider.id] = []
                }
            }
        } catch {
            self.providersError = ChatError.from(error)
        }
    }
}

private struct EmptyStatePane: View {
    let error: ChatError?
    let hasProviders: Bool

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "bubble.left.and.bubble.right")
                .font(.largeTitle)
                .foregroundStyle(.secondary)
            Text("No conversation selected")
                .font(.headline)
            if let error {
                Text(error.message)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
                    .textSelection(.enabled)
            } else if !hasProviders {
                Text("Configure a provider in Settings to start a conversation.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            } else {
                Text("Click + in the sidebar to start a new conversation.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(24)
    }
}
