import Foundation

/// Coordinator for the conversation sidebar.
///
/// Holds the list of conversations rendered in the sidebar, the id of the
/// currently selected one, and a cache of `ChatViewModel`s so concurrent
/// streams in different tabs don't lose state when the user switches
/// between them.
///
/// Hexagonal: imports Domain only (gateway protocols, model types). The
/// view binds via `@Observable` to the published state.
@MainActor
@Observable
public final class ConversationTabsViewModel {
    public private(set) var conversations: [Conversation] = []
    public private(set) var activeConversationId: String? = nil
    public private(set) var error: ChatError? = nil
    public private(set) var isLoading: Bool = false
    public private(set) var sandboxTemplates: [SandboxTemplate] = []
    public private(set) var didLoadOnce: Bool = false

    /// Reachable when the gateway returned an empty list AND we've
    /// completed at least one load. Distinct from "loading" (no data
    /// yet) and "errored" (error has a value).
    public var isEmpty: Bool {
        didLoadOnce && conversations.isEmpty && error == nil
    }

    public func clearError() {
        error = nil
    }

    /// Reload after an error or for a manual refresh. Same as `load()`
    /// but renamed for the view's "Try again" affordance.
    public func reload() async {
        error = nil
        await load()
    }

    private let conversationGateway: ConversationGateway
    private let messageGateway: MessageGateway
    private let runGateway: RunGateway
    private let providersGateway: ProvidersGateway
    private let sandboxGateway: SandboxTemplatesGateway

    private var chatViewModelCache: [String: ChatViewModel] = [:]

    public init(
        conversationGateway: ConversationGateway,
        messageGateway: MessageGateway,
        runGateway: RunGateway,
        providersGateway: ProvidersGateway,
        sandboxGateway: SandboxTemplatesGateway
    ) {
        self.conversationGateway = conversationGateway
        self.messageGateway = messageGateway
        self.runGateway = runGateway
        self.providersGateway = providersGateway
        self.sandboxGateway = sandboxGateway
    }

    /// Load the conversation list and (best-effort) sandbox templates in
    /// parallel. Selects the most recently updated conversation if there
    /// is none currently active.
    public func load() async {
        isLoading = true
        defer {
            isLoading = false
            didLoadOnce = true
        }
        async let conversationsResult = loadConversations()
        async let templatesResult = loadSandboxTemplates()
        await conversationsResult
        await templatesResult
    }

    private func loadConversations() async {
        do {
            let page = try await conversationGateway.list(limit: nil, cursor: nil)
            self.conversations = page.conversations
            if activeConversationId == nil {
                activeConversationId = page.conversations.first?.id
            } else if !page.conversations.contains(where: { $0.id == activeConversationId }) {
                activeConversationId = page.conversations.first?.id
            }
        } catch {
            self.error = ChatError.from(error)
        }
    }

    private func loadSandboxTemplates() async {
        do {
            self.sandboxTemplates = try await sandboxGateway.list()
        } catch {
            // Sandbox is optional surface here; keep empty rather than
            // failing the whole load.
            self.sandboxTemplates = []
        }
    }

    public func select(_ conversationId: String) {
        guard conversations.contains(where: { $0.id == conversationId }) else { return }
        activeConversationId = conversationId
    }

    /// Creates a conversation via the gateway, prepends it, and selects it.
    /// Hydrates the new tab's `ChatViewModel` so the caller sees an empty
    /// chat ready for input.
    public func createConversation(
        providerId: String,
        model: String,
        title: String? = nil,
        sandboxTemplateId: String? = nil
    ) async {
        let request = CreateConversationRequest(
            providerId: providerId,
            model: model,
            title: title,
            sandboxTemplateId: sandboxTemplateId
        )
        do {
            let created = try await conversationGateway.create(request)
            conversations.insert(created, at: 0)
            activeConversationId = created.id
        } catch {
            self.error = ChatError.from(error)
        }
    }

    public func rename(_ conversationId: String, to title: String) async {
        let trimmed = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        do {
            let updated = try await conversationGateway.patch(
                id: conversationId,
                PatchConversationRequest(title: trimmed)
            )
            if let idx = conversations.firstIndex(where: { $0.id == conversationId }) {
                conversations[idx] = updated
            }
        } catch {
            self.error = ChatError.from(error)
        }
    }

    public func delete(_ conversationId: String) async {
        let wasActive = (activeConversationId == conversationId)
        do {
            try await conversationGateway.delete(id: conversationId)
            conversations.removeAll(where: { $0.id == conversationId })
            chatViewModelCache.removeValue(forKey: conversationId)
            if wasActive {
                activeConversationId = conversations.first?.id
            }
        } catch {
            self.error = ChatError.from(error)
        }
    }

    /// Returns a cached `ChatViewModel` for the conversation, or builds and
    /// caches a fresh one on first access. Caching is essential: streaming
    /// runs must continue across tab switches without losing state.
    public func chatViewModel(for conversationId: String) -> ChatViewModel {
        if let cached = chatViewModelCache[conversationId] {
            return cached
        }
        let vm = ChatViewModel(
            runGateway: runGateway,
            messageGateway: messageGateway,
            conversationId: conversationId
        )
        chatViewModelCache[conversationId] = vm
        return vm
    }

    /// Test seam: pre-seeded ChatViewModel cache lets tests observe state
    /// after stream events fire without rebuilding the gateway plumbing.
    func _setCachedChatViewModel(_ vm: ChatViewModel, for conversationId: String) {
        chatViewModelCache[conversationId] = vm
    }
}
