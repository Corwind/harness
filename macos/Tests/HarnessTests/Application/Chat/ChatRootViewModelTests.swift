import XCTest
@testable import HarnessApp

@MainActor
final class ChatRootViewModelTests: XCTestCase {
    func testBootstrapPicksExistingConversationAndHydratesHistory() async {
        let runs = FakeRunGateway(script: [])
        let messages = FakeMessageGateway()
        messages.stubbedHistory = [
            Message(
                id: "m1", conversationId: "conv_1", role: .user,
                content: [.text(TextBlock(text: "earlier"))],
                createdAt: "t", ordinal: 0
            ),
            Message(
                id: "m2", conversationId: "conv_1", role: .assistant,
                content: [.text(TextBlock(text: "earlier reply"))],
                createdAt: "t", ordinal: 1
            ),
        ]
        let conversations = FakeConversationGateway(seeded: [
            Conversation(
                id: "conv_1", title: "Old chat", providerId: "claude",
                model: "claude-3", sandboxTemplateId: nil,
                createdAt: "t", updatedAt: "t"
            )
        ])
        let providers = makeFakeProviders(
            providers: [makeProvider(id: "claude", configured: true)],
            models: ["claude": [Model(id: "claude-3-sonnet", displayName: "Sonnet")]]
        )

        let root = ChatRootViewModel(
            runGateway: runs,
            messageGateway: messages,
            conversationGateway: conversations,
            providersGateway: providers
        )

        await root.bootstrap()

        guard case .ready(let chatVM) = root.state else {
            return XCTFail("expected ready state, got \(root.state)")
        }
        XCTAssertEqual(chatVM.messages.count, 2)
        XCTAssertEqual(chatVM.messages.first?.text, "earlier")
        XCTAssertEqual(chatVM.messages.last?.text, "earlier reply")
        XCTAssertEqual(messages.listed.first?.conversationId, "conv_1")
        XCTAssertTrue(conversations.created.isEmpty, "should not create when one already exists")
    }

    func testBootstrapCreatesConversationWhenNoneExists() async {
        let runs = FakeRunGateway(script: [])
        let messages = FakeMessageGateway()
        let conversations = FakeConversationGateway(seeded: [])
        let providers = makeFakeProviders(
            providers: [makeProvider(id: "claude", configured: true)],
            models: ["claude": [Model(id: "claude-3-sonnet", displayName: "Sonnet")]]
        )

        let root = ChatRootViewModel(
            runGateway: runs,
            messageGateway: messages,
            conversationGateway: conversations,
            providersGateway: providers
        )

        await root.bootstrap()

        guard case .ready = root.state else {
            return XCTFail("expected ready state, got \(root.state)")
        }
        XCTAssertEqual(conversations.created.count, 1)
        XCTAssertEqual(conversations.created[0].providerId, "claude")
        XCTAssertEqual(conversations.created[0].model, "claude-3-sonnet")
    }

    func testBootstrapFailsWhenNoProvidersAvailable() async {
        let runs = FakeRunGateway(script: [])
        let messages = FakeMessageGateway()
        let conversations = FakeConversationGateway(seeded: [])
        let providers = makeFakeProviders(providers: [], models: [:])

        let root = ChatRootViewModel(
            runGateway: runs,
            messageGateway: messages,
            conversationGateway: conversations,
            providersGateway: providers
        )

        await root.bootstrap()

        guard case .failed(let err) = root.state else {
            return XCTFail("expected failed state, got \(root.state)")
        }
        XCTAssertEqual(err.code, "no_provider")
    }

    func testReloadDoesNotMixHistoryWithLiveStream() async {
        // After history is hydrated, sending a new message should not lose
        // the historical entries; the user-message append must come *after*
        // the existing tail.
        let messages = FakeMessageGateway(nextRunId: "r-live")
        messages.stubbedHistory = [
            Message(
                id: "m1", conversationId: "conv_1", role: .user,
                content: [.text(TextBlock(text: "old user"))],
                createdAt: "t", ordinal: 0
            ),
            Message(
                id: "m2", conversationId: "conv_1", role: .assistant,
                content: [.text(TextBlock(text: "old reply"))],
                createdAt: "t", ordinal: 1
            ),
        ]
        let runs = FakeRunGateway(script: [
            .event(.runStart(.init(runId: "r-live", conversationId: "conv_1", startedAt: "t"))),
            .event(.messageStart(.init(id: "m3"))),
            .event(.contentDelta(.init(text: "live"))),
            .event(.messageStop(.init(stopReason: .endTurn, usage: nil))),
            .event(.runEnd(.init(runId: "r-live", status: .completed, endedAt: "t"))),
        ])
        let conversations = FakeConversationGateway(seeded: [
            Conversation(
                id: "conv_1", title: "k", providerId: "claude", model: "c",
                sandboxTemplateId: nil, createdAt: "t", updatedAt: "t"
            )
        ])
        let providers = makeFakeProviders(
            providers: [makeProvider(id: "claude", configured: true)],
            models: ["claude": [Model(id: "c", displayName: "C")]]
        )

        let root = ChatRootViewModel(
            runGateway: runs, messageGateway: messages,
            conversationGateway: conversations, providersGateway: providers
        )
        await root.bootstrap()

        guard case .ready(let chatVM) = root.state else {
            return XCTFail("expected ready, got \(root.state)")
        }

        await chatVM.send("new question")

        XCTAssertEqual(chatVM.messages.count, 4)
        XCTAssertEqual(chatVM.messages[0].text, "old user")
        XCTAssertEqual(chatVM.messages[1].text, "old reply")
        XCTAssertEqual(chatVM.messages[2].role, MessageRole.user)
        XCTAssertEqual(chatVM.messages[2].text, "new question")
        XCTAssertEqual(chatVM.messages[3].role, MessageRole.assistant)
        XCTAssertEqual(chatVM.messages[3].text, "live")
    }

    private func makeProvider(id: String, configured: Bool) -> Provider {
        Provider(
            id: id,
            displayName: id.capitalized,
            configured: configured,
            capabilities: ProviderCapabilities(
                streaming: true, tools: true, vision: false,
                systemPrompt: true, maxContextTokens: 8192
            )
        )
    }

    private func makeFakeProviders(
        providers: [Provider],
        models: [String: [Model]]
    ) -> FakeProvidersGateway {
        let g = FakeProvidersGateway()
        g.listResult = .success(providers)
        // Snapshot: matches the first lookup's provider, since the fake's
        // modelsResult is a single Result and not a per-provider map. Tests
        // only ever resolve one provider, so this is sufficient.
        if let first = providers.first, let modelList = models[first.id] {
            g.modelsResult = .success(modelList)
        }
        return g
    }
}

final class FakeConversationGateway: ConversationGateway, @unchecked Sendable {
    private let lock = NSLock()
    private var seeded: [Conversation]
    public private(set) var created: [CreateConversationRequest] = []

    init(seeded: [Conversation]) {
        self.seeded = seeded
    }

    func list(limit: Int?, cursor: String?) async throws -> ConversationsPage {
        lock.lock(); defer { lock.unlock() }
        return ConversationsPage(conversations: seeded, nextCursor: nil)
    }

    func create(_ request: CreateConversationRequest) async throws -> Conversation {
        lock.lock(); defer { lock.unlock() }
        created.append(request)
        let conv = Conversation(
            id: "conv_new_\(created.count)",
            title: request.title ?? "Untitled",
            providerId: request.providerId,
            model: request.model,
            sandboxTemplateId: request.sandboxTemplateId,
            createdAt: "t", updatedAt: "t"
        )
        seeded.append(conv)
        return conv
    }

    func get(id: String) async throws -> Conversation {
        lock.lock(); defer { lock.unlock() }
        guard let c = seeded.first(where: { $0.id == id }) else {
            throw ChatError(code: "not_found", message: id)
        }
        return c
    }

    func patch(id: String, _ request: PatchConversationRequest) async throws -> Conversation {
        try await get(id: id)
    }

    func delete(id: String) async throws {
        lock.lock(); defer { lock.unlock() }
        seeded.removeAll(where: { $0.id == id })
    }
}

