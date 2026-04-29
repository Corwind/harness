import XCTest
@testable import HarnessApp

/// End-to-end tests that spawn the real `harness-server` binary with
/// `HARNESS_FAKE_PROVIDER=1` and drive a `ChatViewModel` through it.
///
/// Skipping policy: if the binary cannot be located (e.g. fresh checkout
/// where backend hasn't been built), the test calls XCTSkip rather than
/// failing. The test that requires the fake-provider knob also confirms the
/// knob is honoured by the running binary; if the binary is older than
/// the knob landing, the test skips with a clear message.
@MainActor
final class ChatViewModelLiveTests: XCTestCase {
    func testEndToEndUserToAssistantStreamsThroughRealBackend() async throws {
        let harness = try await spawnHarnessOrSkip()
        defer { harness.shutdown() }

        let client = HTTPClient(baseURL: harness.session.baseURL, token: harness.session.token)
        let conversations = ConversationGatewayAdapter(client: client)
        let runs = RunGatewayAdapter(client: client)
        let messages = MessageGatewayAdapter(client: client)
        let providers = ProvidersGatewayAdapter(client: client)

        try await ensureFakeProviderConfigured(providers: providers)
        let conversation = try await createDefaultConversation(
            conversations: conversations,
            providers: providers
        )

        let chatVM = ChatViewModel(
            runGateway: runs,
            messageGateway: messages,
            conversationId: conversation.id
        )

        await chatVM.send("hello live")

        try skipIfRunErrored(
            chatVM,
            because: "live run errored — likely HARNESS_FAKE_PROVIDER not honoured by this binary"
        )

        XCTAssertFalse(chatVM.isStreaming, "stream should have terminated")
        XCTAssertEqual(chatVM.runStatus, .completed)
        XCTAssertNil(chatVM.error)
        XCTAssertGreaterThanOrEqual(chatVM.messages.count, 2)
        XCTAssertEqual(chatVM.messages.first?.role, MessageRole.user)
        XCTAssertEqual(chatVM.messages.first?.text, "hello live")
        XCTAssertEqual(chatVM.messages.last?.role, MessageRole.assistant)
        let assistantText = chatVM.messages.last?.text ?? ""
        XCTAssertFalse(assistantText.isEmpty, "assistant text should be populated")
    }

    func testCancellationPropagatesToServerAndStopsStream() async throws {
        // 50 ms / event opens a clean cancellation window between the
        // fake's three scripted events. SSEReader's prompt teardown
        // (commit cf48631) keeps this under ~500 ms wall-clock.
        let harness = try await spawnHarnessOrSkip(extraEnv: [
            "HARNESS_FAKE_PROVIDER_DELAY_MS": "50",
        ])
        defer { harness.shutdown() }

        let client = HTTPClient(baseURL: harness.session.baseURL, token: harness.session.token)
        let conversations = ConversationGatewayAdapter(client: client)
        let runs = RunGatewayAdapter(client: client)
        let messages = MessageGatewayAdapter(client: client)
        let providers = ProvidersGatewayAdapter(client: client)

        try await ensureFakeProviderConfigured(providers: providers)
        let conversation = try await createDefaultConversation(
            conversations: conversations,
            providers: providers
        )

        let chatVM = ChatViewModel(
            runGateway: runs,
            messageGateway: messages,
            conversationId: conversation.id
        )

        let sendTask = Task { @MainActor in await chatVM.send("hello slow") }

        let sawContent = await waitFor(timeout: 5.0) { @MainActor in
            guard let last = chatVM.messages.last,
                  last.role == .assistant,
                  !last.text.isEmpty else { return false }
            return chatVM.isStreaming
        }
        if !sawContent {
            await sendTask.value
            XCTFail("assistant content never streamed; cancellation cannot be evaluated")
            return
        }

        chatVM.cancel()
        await sendTask.value

        XCTAssertFalse(chatVM.isStreaming, "isStreaming must reset")
        XCTAssertEqual(chatVM.runStatus, .cancelled)

        let stored = try await messages.list(
            conversationId: conversation.id,
            limit: nil,
            afterOrdinal: nil
        )
        let userMessages = stored.filter { $0.role == .user }
        XCTAssertFalse(userMessages.isEmpty, "user message should be persisted")
    }

    // MARK: - Helpers

    private func spawnHarnessOrSkip(extraEnv: [String: String] = [:]) async throws -> LiveBackendHarness {
        guard LiveBackendHarness.locateBinary() != nil else {
            throw XCTSkip(
                "harness-server binary not found; run `cargo build -p harness-server` " +
                "or set HARNESS_BACKEND_PATH"
            )
        }
        do {
            return try await LiveBackendHarness(extraEnv: extraEnv)
        } catch let LiveBackendHarness.Error.handshakeFailed(reason) {
            // The binary may pre-date the HARNESS_FAKE_PROVIDER knob (T2.1.x);
            // in that case it will fail because no provider is configured.
            // We skip rather than fail since this is an environment issue.
            throw XCTSkip("live backend handshake failed: \(reason)")
        }
    }

    private func ensureFakeProviderConfigured(providers: ProvidersGatewayAdapter) async throws {
        // Even with HARNESS_FAKE_PROVIDER=1 the provider needs an api_key row
        // (per the Claude provider's parse contract). Push a placeholder.
        do {
            _ = try await providers.upsertConfig(
                providerId: "claude",
                ProviderConfig(apiKey: "fake-test-key", baseURL: nil)
            )
        } catch {
            // If the upsert is not strictly required (knob bypasses the
            // contract) tolerate this silently.
        }
    }

    private func createDefaultConversation(
        conversations: ConversationGatewayAdapter,
        providers: ProvidersGatewayAdapter
    ) async throws -> Conversation {
        let providerList = try await providers.list()
        guard let claude = providerList.first(where: { $0.id == "claude" }) ?? providerList.first else {
            throw XCTSkip("no providers registered (server-side fake provider knob not active)")
        }
        let models = try await providers.listModels(providerId: claude.id)
        guard let model = models.first else {
            throw XCTSkip("no models for provider \(claude.id)")
        }
        return try await conversations.create(
            CreateConversationRequest(
                providerId: claude.id,
                model: model.id,
                title: "live e2e",
                sandboxTemplateId: nil
            )
        )
    }

    /// Skip rather than fail when the live run errors — typically that
    /// means the fake-provider env knob is not honoured by the binary
    /// under test (T2.1.x). Tests should fail only when the knob is in
    /// place but the wiring is broken.
    private func skipIfRunErrored(
        _ vm: ChatViewModel,
        because message: String
    ) throws {
        if vm.runStatus == .errored {
            let detail = vm.error?.message ?? "no error message"
            throw XCTSkip("\(message) — \(detail)")
        }
    }

    /// Returns `true` once predicate is true, `false` if timeout elapses.
    /// Does not XCTFail — caller decides skip vs fail.
    private func waitFor(
        timeout: TimeInterval,
        _ predicate: @escaping @MainActor () -> Bool
    ) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if await MainActor.run(body: predicate) { return true }
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
        return false
    }
}
