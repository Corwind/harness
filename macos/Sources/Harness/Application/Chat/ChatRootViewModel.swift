import Foundation

/// Loading state of the chat root: we need to resolve a default conversation
/// (or create one) before the chat view model can be constructed.
public enum ChatRootState: Sendable {
    case loading
    case ready(ChatViewModel)
    case failed(ChatError)
}

/// Application-layer coordinator that resolves a conversation to chat in,
/// then stands up the [`ChatViewModel`] for it. The composition root passes
/// in concrete gateways; this type owns the policy for "which conversation
/// do we open on launch".
@MainActor
@Observable
public final class ChatRootViewModel {
    public private(set) var state: ChatRootState = .loading

    private let runGateway: RunGateway
    private let messageGateway: MessageGateway
    private let conversationGateway: ConversationGateway
    private let providersGateway: ProvidersGateway
    private let defaults: DefaultsResolver

    public init(
        runGateway: RunGateway,
        messageGateway: MessageGateway,
        conversationGateway: ConversationGateway,
        providersGateway: ProvidersGateway,
        defaults: DefaultsResolver = DefaultsResolver()
    ) {
        self.runGateway = runGateway
        self.messageGateway = messageGateway
        self.conversationGateway = conversationGateway
        self.providersGateway = providersGateway
        self.defaults = defaults
    }

    /// Fetch the most recent conversation (or create one) and hydrate the
    /// resulting `ChatViewModel`'s history before exposing it.
    public func bootstrap() async {
        state = .loading
        do {
            let conversation = try await resolveConversation()
            let chatVM = ChatViewModel(
                runGateway: runGateway,
                messageGateway: messageGateway,
                conversationId: conversation.id
            )
            await chatVM.loadHistory()
            state = .ready(chatVM)
        } catch {
            state = .failed(ChatError.from(error))
        }
    }

    private func resolveConversation() async throws -> Conversation {
        let page = try await conversationGateway.list(limit: 1, cursor: nil)
        if let existing = page.conversations.first {
            return existing
        }
        let request = try await defaults.defaultCreateRequest(
            providersGateway: providersGateway
        )
        return try await conversationGateway.create(request)
    }
}

/// Pure helper that picks a default `(provider_id, model)` for a brand-new
/// conversation when there is no existing one. Hexagonal: depends only on
/// the providers gateway protocol.
public struct DefaultsResolver: Sendable {
    public init() {}

    public func defaultCreateRequest(
        providersGateway: ProvidersGateway
    ) async throws -> CreateConversationRequest {
        let providers = try await providersGateway.list()
        guard let provider = providers.first(where: { $0.configured }) ?? providers.first else {
            throw ChatError(
                code: "no_provider",
                message: "No provider is configured. Open Settings to add one."
            )
        }
        let models = try await providersGateway.listModels(providerId: provider.id)
        guard let model = models.first else {
            throw ChatError(
                code: "no_model",
                message: "Provider \(provider.displayName) has no models available."
            )
        }
        return CreateConversationRequest(
            providerId: provider.id,
            model: model.id,
            title: "New conversation",
            sandboxTemplateId: nil
        )
    }
}
