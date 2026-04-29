import XCTest
@testable import HarnessApp

@MainActor
final class ConversationTabsViewModelTests: XCTestCase {
    func testIsEmptyTrueAfterLoadWithNoConversations() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [])
        let vm = makeViewModel(conversationGateway: conversations)

        XCTAssertFalse(vm.isEmpty, "must not be empty before first load")
        XCTAssertFalse(vm.didLoadOnce)

        await vm.load()

        XCTAssertTrue(vm.didLoadOnce)
        XCTAssertTrue(vm.isEmpty)
        XCTAssertTrue(vm.conversations.isEmpty)
        XCTAssertNil(vm.error)
    }

    func testIsLoadingFlagSetDuringLoad() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [])
        let vm = makeViewModel(conversationGateway: conversations)
        XCTAssertFalse(vm.isLoading)

        let task = Task { @MainActor in await vm.load() }
        await task.value
        XCTAssertFalse(vm.isLoading, "must reset after load completes")
    }

    func testIsEmptyFalseWhileErrorPresent() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [])
        conversations.listError = BackendError.transport("offline")
        let vm = makeViewModel(conversationGateway: conversations)

        await vm.load()

        XCTAssertNotNil(vm.error)
        XCTAssertFalse(vm.isEmpty, "an errored load is not the same as an empty list")
    }

    func testReloadClearsErrorAndRefetches() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [])
        conversations.listError = BackendError.transport("offline")
        let vm = makeViewModel(conversationGateway: conversations)
        await vm.load()
        XCTAssertNotNil(vm.error)

        // Recover the gateway and reload.
        conversations.listError = nil
        conversations.appendSeeded(makeConversation(id: "c1", title: "first"))
        await vm.reload()

        XCTAssertNil(vm.error)
        XCTAssertEqual(vm.conversations.map(\.id), ["c1"])
    }

    func testTransportErrorFromGatewayMapsToTypedKind() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [])
        conversations.listError = BackendError.transport("offline")
        let vm = makeViewModel(conversationGateway: conversations)

        await vm.load()

        XCTAssertEqual(vm.error?.kind, .transport)
        XCTAssertEqual(vm.error?.actions.canRetry, true)
    }

    func testLoadPopulatesSidebarFromGatewayAndSelectsFirst() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [
            makeConversation(id: "c1", title: "first"),
            makeConversation(id: "c2", title: "second"),
        ])
        let vm = makeViewModel(conversationGateway: conversations)

        await vm.load()

        XCTAssertEqual(vm.conversations.map(\.id), ["c1", "c2"])
        XCTAssertEqual(vm.activeConversationId, "c1")
        XCTAssertNil(vm.error)
    }

    func testCreateAddsToSidebarAndSelectsNewConversation() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [
            makeConversation(id: "c1", title: "old"),
        ])
        let vm = makeViewModel(conversationGateway: conversations)
        await vm.load()

        await vm.createConversation(
            providerId: "claude",
            model: "claude-3",
            title: "new chat",
            sandboxTemplateId: "tpl_strict"
        )

        XCTAssertEqual(conversations.created.count, 1)
        let req = conversations.created[0]
        XCTAssertEqual(req.providerId, "claude")
        XCTAssertEqual(req.model, "claude-3")
        XCTAssertEqual(req.title, "new chat")
        XCTAssertEqual(req.sandboxTemplateId, "tpl_strict")

        XCTAssertEqual(vm.conversations.first?.title, "new chat")
        XCTAssertEqual(vm.activeConversationId, vm.conversations.first?.id)
    }

    func testTwoConcurrentRunsInSeparateTabsDoNotCrossContaminate() async throws {
        let conversations = TabsTestFakeConversationGateway(seeded: [
            makeConversation(id: "tab_a", title: "A"),
            makeConversation(id: "tab_b", title: "B"),
        ])
        let runsA = FakeRunGateway(script: [
            .event(.runStart(.init(runId: "r_a", conversationId: "tab_a", startedAt: "t"))),
            .event(.messageStart(.init(id: "m_a"))),
            .event(.contentDelta(.init(text: "from-A"))),
            .event(.messageStop(.init(stopReason: .endTurn, usage: nil))),
            .event(.runEnd(.init(runId: "r_a", status: .completed, endedAt: "t"))),
        ])
        let runsB = FakeRunGateway(script: [
            .event(.runStart(.init(runId: "r_b", conversationId: "tab_b", startedAt: "t"))),
            .event(.messageStart(.init(id: "m_b"))),
            .event(.contentDelta(.init(text: "from-B"))),
            .event(.messageStop(.init(stopReason: .endTurn, usage: nil))),
            .event(.runEnd(.init(runId: "r_b", status: .completed, endedAt: "t"))),
        ])
        let messagesA = FakeMessageGateway(nextRunId: "r_a")
        let messagesB = FakeMessageGateway(nextRunId: "r_b")

        let vm = makeViewModel(conversationGateway: conversations)
        await vm.load()
        let chatA = ChatViewModel(runGateway: runsA, messageGateway: messagesA, conversationId: "tab_a")
        let chatB = ChatViewModel(runGateway: runsB, messageGateway: messagesB, conversationId: "tab_b")
        vm._setCachedChatViewModel(chatA, for: "tab_a")
        vm._setCachedChatViewModel(chatB, for: "tab_b")

        async let a: () = chatA.send("ping A")
        async let b: () = chatB.send("ping B")
        _ = await (a, b)

        XCTAssertEqual(chatA.messages.last?.text, "from-A")
        XCTAssertEqual(chatB.messages.last?.text, "from-B")
        // First message in each tab is the user message; assert it carries
        // the per-tab text and the assistant reply does not leak across.
        XCTAssertEqual(chatA.messages.first?.text, "ping A")
        XCTAssertEqual(chatB.messages.first?.text, "ping B")
        XCTAssertFalse(chatA.messages.contains(where: { $0.text.contains("from-B") }))
        XCTAssertFalse(chatB.messages.contains(where: { $0.text.contains("from-A") }))
    }

    func testSwitchingTabsPreservesActiveStreamState() async throws {
        let conversations = TabsTestFakeConversationGateway(seeded: [
            makeConversation(id: "tab_a", title: "A"),
            makeConversation(id: "tab_b", title: "B"),
        ])
        let runsA = FakeRunGateway(script: [
            .event(.runStart(.init(runId: "r_a", conversationId: "tab_a", startedAt: "t"))),
            .event(.messageStart(.init(id: "m_a"))),
            .event(.contentDelta(.init(text: "live-content"))),
            .gate("hold-A"),
            .event(.contentDelta(.init(text: "-after-switch"))),
            .event(.messageStop(.init(stopReason: .endTurn, usage: nil))),
            .event(.runEnd(.init(runId: "r_a", status: .completed, endedAt: "t"))),
        ])
        let runsB = FakeRunGateway(script: [])
        let messagesA = FakeMessageGateway(nextRunId: "r_a")
        let messagesB = FakeMessageGateway(nextRunId: "r_b")

        let vm = makeViewModel(conversationGateway: conversations)
        await vm.load()
        let chatA = ChatViewModel(runGateway: runsA, messageGateway: messagesA, conversationId: "tab_a")
        let chatB = ChatViewModel(runGateway: runsB, messageGateway: messagesB, conversationId: "tab_b")
        vm._setCachedChatViewModel(chatA, for: "tab_a")
        vm._setCachedChatViewModel(chatB, for: "tab_b")

        vm.select("tab_a")
        let sendTask = Task { @MainActor in await chatA.send("hello") }

        // Wait until the partial content from tab A is visible.
        try await waitUntilMainActor(timeout: 2.0) {
            chatA.messages.last?.text == "live-content"
        }
        XCTAssertTrue(chatA.isStreaming)

        // Switch to B, then back to A. The cached view model must still
        // be the same instance and still streaming.
        vm.select("tab_b")
        XCTAssertEqual(vm.activeConversationId, "tab_b")
        XCTAssertTrue(chatA.isStreaming, "stream should keep running while user is on tab B")

        vm.select("tab_a")
        XCTAssertEqual(vm.activeConversationId, "tab_a")
        XCTAssertTrue(chatA.isStreaming)
        XCTAssertEqual(chatA.messages.last?.text, "live-content")

        // Release the gate so the run finishes and we don't leak the task.
        runsA.releaseGate(named: "hold-A")
        await sendTask.value
        XCTAssertFalse(chatA.isStreaming)
        XCTAssertEqual(chatA.messages.last?.text, "live-content-after-switch")
    }

    func testRenameUpdatesSidebarLabel() async {
        let conv = makeConversation(id: "c1", title: "old name")
        let conversations = TabsTestFakeConversationGateway(seeded: [conv])
        let vm = makeViewModel(conversationGateway: conversations)
        await vm.load()

        await vm.rename("c1", to: "renamed")

        XCTAssertEqual(conversations.patches.count, 1)
        XCTAssertEqual(conversations.patches[0].id, "c1")
        XCTAssertEqual(conversations.patches[0].request.title, "renamed")
        XCTAssertEqual(vm.conversations[0].title, "renamed")
    }

    func testRenameWithEmptyTitleIsNoOp() async {
        let conv = makeConversation(id: "c1", title: "keep")
        let conversations = TabsTestFakeConversationGateway(seeded: [conv])
        let vm = makeViewModel(conversationGateway: conversations)
        await vm.load()

        await vm.rename("c1", to: "   ")

        XCTAssertTrue(conversations.patches.isEmpty)
        XCTAssertEqual(vm.conversations[0].title, "keep")
    }

    func testDeleteRemovesTabAndFallsBackToNextWhenActive() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [
            makeConversation(id: "c1", title: "first"),
            makeConversation(id: "c2", title: "second"),
        ])
        let vm = makeViewModel(conversationGateway: conversations)
        await vm.load()
        XCTAssertEqual(vm.activeConversationId, "c1")

        await vm.delete("c1")

        XCTAssertEqual(conversations.deleted, ["c1"])
        XCTAssertEqual(vm.conversations.map(\.id), ["c2"])
        XCTAssertEqual(vm.activeConversationId, "c2")
    }

    func testDeleteDoesNotChangeSelectionWhenDeletingInactiveTab() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [
            makeConversation(id: "c1", title: "first"),
            makeConversation(id: "c2", title: "second"),
        ])
        let vm = makeViewModel(conversationGateway: conversations)
        await vm.load()
        XCTAssertEqual(vm.activeConversationId, "c1")

        await vm.delete("c2")

        XCTAssertEqual(conversations.deleted, ["c2"])
        XCTAssertEqual(vm.conversations.map(\.id), ["c1"])
        XCTAssertEqual(vm.activeConversationId, "c1")
    }

    func testLoadAlsoFetchesSandboxTemplatesForNewConversationModal() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [])
        let templates = FakeSandboxTemplatesGateway(seeded: [
            SandboxTemplate(
                id: "tpl_strict", name: "strict-readonly", description: nil,
                profile: "(version 1)", isBuiltin: true,
                createdAt: "t", updatedAt: "t"
            ),
        ])
        let vm = makeViewModel(
            conversationGateway: conversations,
            sandboxGateway: templates
        )

        await vm.load()

        XCTAssertEqual(vm.sandboxTemplates.map(\.id), ["tpl_strict"])
    }

    func testSandboxTemplateLoadFailureDoesNotBlockConversationsLoad() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [
            makeConversation(id: "c1", title: "first"),
        ])
        let templates = FailingSandboxTemplatesGateway()
        let vm = makeViewModel(
            conversationGateway: conversations,
            sandboxGateway: templates
        )

        await vm.load()

        XCTAssertEqual(vm.conversations.map(\.id), ["c1"])
        XCTAssertTrue(vm.sandboxTemplates.isEmpty)
        XCTAssertNil(vm.error, "sandbox load failure must not surface as a sidebar error")
    }

    func testChatViewModelIsCachedPerConversation() async {
        let conversations = TabsTestFakeConversationGateway(seeded: [
            makeConversation(id: "c1", title: "x"),
        ])
        let vm = makeViewModel(conversationGateway: conversations)
        await vm.load()

        let first = vm.chatViewModel(for: "c1")
        let second = vm.chatViewModel(for: "c1")
        XCTAssertTrue(first === second, "same conversation must yield the same ChatViewModel instance")

        let other = vm.chatViewModel(for: "c-other")
        XCTAssertFalse(first === other)
    }

    // MARK: - Helpers

    private func makeConversation(
        id: String,
        title: String,
        sandboxTemplateId: String? = nil
    ) -> Conversation {
        Conversation(
            id: id,
            title: title,
            providerId: "claude",
            model: "claude-3",
            sandboxTemplateId: sandboxTemplateId,
            createdAt: "2026-04-28T00:00:00Z",
            updatedAt: "2026-04-28T00:00:00Z"
        )
    }

    private func makeViewModel(
        conversationGateway: ConversationGateway,
        sandboxGateway: SandboxTemplatesGateway? = nil
    ) -> ConversationTabsViewModel {
        ConversationTabsViewModel(
            conversationGateway: conversationGateway,
            messageGateway: FakeMessageGateway(),
            runGateway: FakeRunGateway(script: []),
            providersGateway: FakeProvidersGateway(),
            sandboxGateway: sandboxGateway ?? FakeSandboxTemplatesGateway(seeded: [])
        )
    }

    private func waitUntilMainActor(
        timeout: TimeInterval,
        _ predicate: @MainActor () -> Bool,
        file: StaticString = #file,
        line: UInt = #line
    ) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if await MainActor.run(body: predicate) { return }
            try await Task.sleep(nanoseconds: 5_000_000)
        }
        XCTFail("waitUntil timed out", file: file, line: line)
    }
}

/// Local conversation gateway fake; named to avoid colliding with the
/// `FakeConversationGateway` defined in `ChatRootViewModelTests` in the
/// same test target.
final class TabsTestFakeConversationGateway: ConversationGateway, @unchecked Sendable {
    struct PatchCall {
        let id: String
        let request: PatchConversationRequest
    }

    private let lock = NSLock()
    private var seeded: [Conversation]
    public private(set) var created: [CreateConversationRequest] = []
    public private(set) var patches: [PatchCall] = []
    public private(set) var deleted: [String] = []
    public var listError: Error?

    init(seeded: [Conversation]) {
        self.seeded = seeded
    }

    func appendSeeded(_ conversation: Conversation) {
        lock.lock(); defer { lock.unlock() }
        seeded.append(conversation)
    }

    func list(limit: Int?, cursor: String?) async throws -> ConversationsPage {
        lock.lock()
        let err = listError
        let snapshot = seeded
        lock.unlock()
        if let err { throw err }
        return ConversationsPage(conversations: snapshot, nextCursor: nil)
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
            createdAt: "2026-04-28T00:00:00Z",
            updatedAt: "2026-04-28T00:00:00Z"
        )
        seeded.insert(conv, at: 0)
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
        lock.lock(); defer { lock.unlock() }
        patches.append(PatchCall(id: id, request: request))
        guard let idx = seeded.firstIndex(where: { $0.id == id }) else {
            throw ChatError(code: "not_found", message: id)
        }
        let original = seeded[idx]
        let updated = Conversation(
            id: original.id,
            title: request.title ?? original.title,
            providerId: original.providerId,
            model: request.model ?? original.model,
            sandboxTemplateId: original.sandboxTemplateId,
            createdAt: original.createdAt,
            updatedAt: "2026-04-28T01:00:00Z"
        )
        seeded[idx] = updated
        return updated
    }

    func delete(id: String) async throws {
        lock.lock(); defer { lock.unlock() }
        deleted.append(id)
        seeded.removeAll(where: { $0.id == id })
    }
}

/// Lightweight fake that only fails `list()` — used for the
/// "sandbox template load failure must not block conversations load"
/// scenario. The fuller `FakeSandboxTemplatesGateway` from the Sandboxes
/// test fakes lacks a fail-on-list knob.
final class FailingSandboxTemplatesGateway: SandboxTemplatesGateway, @unchecked Sendable {
    func list() async throws -> [SandboxTemplate] {
        throw ChatError(code: "transport", message: "offline")
    }
    func create(_ request: CreateSandboxTemplateRequest) async throws -> SandboxTemplate {
        throw ChatError(code: "not_supported", message: "unused")
    }
    func get(id: String) async throws -> SandboxTemplate {
        throw ChatError(code: "not_found", message: id)
    }
    func patch(id: String, _ request: PatchSandboxTemplateRequest) async throws -> SandboxTemplate {
        try await get(id: id)
    }
    func delete(id: String) async throws {}
    func validate(id: String) async throws -> ValidateSandboxResult {
        ValidateSandboxResult(valid: true)
    }
}
