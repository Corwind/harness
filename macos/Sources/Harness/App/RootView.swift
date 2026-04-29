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
        if let activeId = tabsVM.activeConversationId,
           let conversation = tabsVM.conversations.first(where: { $0.id == activeId }) {
            ChatView(
                viewModel: tabsVM.chatViewModel(for: activeId),
                activeSandboxName: sandboxName(for: conversation.sandboxTemplateId),
                actions: chatActions(for: conversation)
            )
            .id(activeId)
        } else if tabsVM.isLoading || !tabsVM.didLoadOnce {
            VStack(spacing: 12) {
                ProgressView()
                Text("Loading conversations…")
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            EmptyStatePane(
                error: tabsVM.error ?? providersError,
                hasProviders: !providers.isEmpty,
                onRetry: { Task { await tabsVM.reload() } }
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func sandboxName(for templateId: String?) -> String? {
        guard let templateId else { return nil }
        return tabsVM.sandboxTemplates.first(where: { $0.id == templateId })?.name
    }

    private func chatActions(for conversation: Conversation) -> ChatViewActions {
        ChatViewActions(
            onOpenProvidersSettings: openProvidersSettings,
            onOpenSandboxesSettings: { templateId in
                openSandboxesSettings(highlight: templateId)
            },
            onChooseSandbox: {
                // T2.5 exposes a sandbox picker in the conversation
                // header; until that picker accepts an external trigger,
                // route the user to Settings → Sandboxes for now.
                openSandboxesSettings(highlight: conversation.sandboxTemplateId)
            }
        )
    }

    private func openProvidersSettings() {
        // SwiftUI Settings scene is presented via the system menu / ⌘,;
        // post a notification the app's settings host can observe to
        // pre-select the Providers tab. Default Settings open is fine
        // until that hook lands.
        NotificationCenter.default.post(
            name: .harnessOpenSettings,
            object: nil,
            userInfo: ["tab": "providers"]
        )
    }

    private func openSandboxesSettings(highlight templateId: String?) {
        var info: [String: Any] = ["tab": "sandboxes"]
        if let templateId { info["highlight"] = templateId }
        NotificationCenter.default.post(
            name: .harnessOpenSettings,
            object: nil,
            userInfo: info
        )
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
    let onRetry: () -> Void

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: error == nil ? "bubble.left.and.bubble.right" : "exclamationmark.triangle.fill")
                .font(.largeTitle)
                .foregroundStyle(error == nil ? Color.secondary : Color.red)
            Text(error == nil ? "No conversation selected" : "Couldn't load conversations")
                .font(.headline)
            if let error {
                Text(error.message)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
                    .textSelection(.enabled)
                if error.actions.canRetry {
                    Button("Try again", action: onRetry)
                        .controlSize(.small)
                }
            } else if !hasProviders {
                Text("Configure a provider in Settings to start a conversation.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            } else {
                Text("No conversations yet. Press ⌘N to create one.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(24)
    }
}

extension Notification.Name {
    static let harnessOpenSettings = Notification.Name("HarnessOpenSettings")
}
