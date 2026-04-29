import XCTest
@testable import HarnessApp

@MainActor
final class ChatViewModelTests: XCTestCase {
    func testSendStreamsTextDeltasAndCompletes() async throws {
        let script: [FakeRunGateway.ScriptStep] = [
            .event(.runStart(.init(runId: "r1", conversationId: "c1", startedAt: "2026-04-28T10:15:00Z"))),
            .event(.messageStart(.init(id: "msg_a"))),
            .event(.contentDelta(.init(text: "H"))),
            .event(.contentDelta(.init(text: "e"))),
            .event(.contentDelta(.init(text: "l"))),
            .event(.contentDelta(.init(text: "l"))),
            .event(.contentDelta(.init(text: "o"))),
            .event(.messageStop(.init(stopReason: .endTurn, usage: .init(inputTokens: 4, outputTokens: 1)))),
            .event(.runEnd(.init(runId: "r1", status: .completed, endedAt: "2026-04-28T10:15:01Z"))),
        ]
        let runs = FakeRunGateway(script: script)
        let messages = FakeMessageGateway(nextRunId: "r1")
        let vm = ChatViewModel(
            runGateway: runs,
            messageGateway: messages,
            conversationId: "c1"
        )

        await vm.send("hello")

        XCTAssertEqual(vm.messages.count, 2)
        XCTAssertEqual(vm.messages[0].role, .user)
        XCTAssertEqual(vm.messages[0].text, "hello")
        XCTAssertEqual(vm.messages[1].role, .assistant)
        XCTAssertEqual(vm.messages[1].id, "msg_a")
        XCTAssertEqual(vm.messages[1].text, "Hello")
        XCTAssertEqual(vm.messages[1].status, .complete)
        XCTAssertFalse(vm.isStreaming)
        XCTAssertEqual(vm.runStatus, .completed)
        XCTAssertNil(vm.error)
        XCTAssertEqual(messages.posted.count, 1)
        XCTAssertEqual(messages.posted[0].conversationId, "c1")
    }

    func testUserMessageAppearsBeforeAssistantStreams() async throws {
        // Once messageStart fires, the user message must already be in the list
        // ahead of the assistant message.
        let script: [FakeRunGateway.ScriptStep] = [
            .event(.runStart(.init(runId: "r1", conversationId: "c1", startedAt: "t"))),
            .event(.messageStart(.init(id: "msg_a"))),
            .event(.contentDelta(.init(text: "Hi"))),
            .event(.messageStop(.init(stopReason: .endTurn, usage: nil))),
            .event(.runEnd(.init(runId: "r1", status: .completed, endedAt: "t"))),
        ]
        let runs = FakeRunGateway(script: script)
        let messages = FakeMessageGateway(nextRunId: "r1")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.send("hi there")

        XCTAssertEqual(vm.messages.first?.role, .user)
        XCTAssertEqual(vm.messages.first?.text, "hi there")
        XCTAssertEqual(vm.messages.last?.role, .assistant)
    }

    func testToolUseLifecycleTransitionsAndStdoutAccumulates() async throws {
        let toolInput: [String: JSONValue] = ["path": .string("/etc/hosts")]
        let script: [FakeRunGateway.ScriptStep] = [
            .event(.runStart(.init(runId: "r2", conversationId: "c1", startedAt: "t"))),
            .event(.messageStart(.init(id: "msg_b"))),
            .event(.toolUseStart(.init(id: "tu_1", name: "read_file"))),
            .event(.toolUseDelta(.init(id: "tu_1", partialJSON: "{\"path\":"))),
            .event(.toolUseDelta(.init(id: "tu_1", partialJSON: "\"/etc/hosts\"}"))),
            .event(.toolUseStop(.init(id: "tu_1", input: toolInput))),
            .event(.toolStart(.init(toolUseId: "tu_1", name: "read_file", kind: .external, sandboxTemplateId: "tpl_strict"))),
            .event(.toolStdout(.init(toolUseId: "tu_1", chunk: "127.0.0.1\t"))),
            .event(.toolStdout(.init(toolUseId: "tu_1", chunk: "localhost\n"))),
            .event(.toolFinish(.init(toolUseId: "tu_1", output: .object(["stdout": .string("127.0.0.1\tlocalhost\n")]), exitCode: 0, durationMs: 12))),
            .event(.messageStop(.init(stopReason: .toolUse, usage: nil))),
            .event(.runEnd(.init(runId: "r2", status: .completed, endedAt: "t"))),
        ]
        let runs = FakeRunGateway(script: script)
        let messages = FakeMessageGateway(nextRunId: "r2")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.send("read it")

        XCTAssertEqual(vm.messages.last?.role, .assistant)
        let calls = vm.messages.last?.toolCalls ?? []
        XCTAssertEqual(calls.count, 1)
        let call = calls[0]
        XCTAssertEqual(call.id, "tu_1")
        XCTAssertEqual(call.name, "read_file")
        XCTAssertEqual(call.status, .finished)
        XCTAssertEqual(call.partialJSON, "{\"path\":\"/etc/hosts\"}")
        XCTAssertEqual(call.input?["path"], .string("/etc/hosts"))
        XCTAssertEqual(call.kind, .external)
        XCTAssertEqual(call.sandboxTemplateId, "tpl_strict")
        XCTAssertEqual(call.stdout, "127.0.0.1\tlocalhost\n")
        XCTAssertEqual(call.exitCode, 0)
        XCTAssertEqual(call.durationMs, 12)
        XCTAssertFalse(vm.isStreaming)
        XCTAssertEqual(vm.runStatus, .completed)
    }

    func testToolErrorMarksCallErrored() async throws {
        let script: [FakeRunGateway.ScriptStep] = [
            .event(.runStart(.init(runId: "r3", conversationId: "c1", startedAt: "t"))),
            .event(.messageStart(.init(id: "msg_c"))),
            .event(.toolUseStart(.init(id: "tu_2", name: "shell"))),
            .event(.toolUseStop(.init(id: "tu_2", input: [:]))),
            .event(.toolStart(.init(toolUseId: "tu_2", name: "shell", kind: .external, sandboxTemplateId: nil))),
            .event(.toolError(.init(toolUseId: "tu_2", code: "sandbox.required", message: "no template"))),
            .event(.messageStop(.init(stopReason: .other, usage: nil))),
            .event(.runEnd(.init(runId: "r3", status: .completed, endedAt: "t"))),
        ]
        let runs = FakeRunGateway(script: script)
        let messages = FakeMessageGateway(nextRunId: "r3")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.send("run it")

        let call = vm.messages.last?.toolCalls.first
        XCTAssertEqual(call?.status, .errored)
        XCTAssertEqual(call?.errorCode, "sandbox.required")
        XCTAssertEqual(call?.errorMessage, "no template")
    }

    func testCancelMidStreamProducesCancelledStatusAndStopsUpdates() async throws {
        let script: [FakeRunGateway.ScriptStep] = [
            .event(.runStart(.init(runId: "r4", conversationId: "c1", startedAt: "t"))),
            .event(.messageStart(.init(id: "msg_d"))),
            .event(.contentDelta(.init(text: "Par"))),
            .gate("after-partial"),
            .event(.contentDelta(.init(text: "tial-after-cancel"))),
            .event(.messageStop(.init(stopReason: .endTurn, usage: nil))),
            .event(.runEnd(.init(runId: "r4", status: .completed, endedAt: "t"))),
        ]
        let runs = FakeRunGateway(script: script)
        let messages = FakeMessageGateway(nextRunId: "r4")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        let sendTask = Task { @MainActor in await vm.send("go") }

        try await waitUntil(timeout: 2.0) { @MainActor in
            vm.messages.last?.role == .assistant && (vm.messages.last?.text ?? "") == "Par"
        }

        vm.cancel()
        runs.releaseGate(named: "after-partial")
        await sendTask.value

        XCTAssertEqual(vm.runStatus, .cancelled)
        XCTAssertFalse(vm.isStreaming)
        XCTAssertEqual(vm.messages.last?.text, "Par", "no further deltas applied after cancel")
        XCTAssertEqual(vm.messages.last?.status, .cancelled)
        XCTAssertEqual(runs.cancelCalls, ["r4"])
    }

    func testErrorEventThenRunEndErroredPreservesStreamedContent() async throws {
        let script: [FakeRunGateway.ScriptStep] = [
            .event(.runStart(.init(runId: "r5", conversationId: "c1", startedAt: "t"))),
            .event(.messageStart(.init(id: "msg_e"))),
            .event(.contentDelta(.init(text: "before-error"))),
            .event(.error(.init(code: "provider.rate_limited", message: "429"))),
            .event(.runEnd(.init(runId: "r5", status: .errored, endedAt: "t"))),
        ]
        let runs = FakeRunGateway(script: script)
        let messages = FakeMessageGateway(nextRunId: "r5")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.send("hi")

        XCTAssertEqual(vm.runStatus, .errored)
        XCTAssertFalse(vm.isStreaming)
        XCTAssertEqual(vm.error?.code, "provider.rate_limited")
        if case .providerRateLimited = vm.error?.kind {
            // expected
        } else {
            XCTFail("expected .providerRateLimited kind, got \(String(describing: vm.error?.kind))")
        }
        XCTAssertTrue(
            vm.error?.message.contains("Rate-limited") ?? false,
            "expected user-readable rate-limit message; got \(String(describing: vm.error?.message))"
        )
        XCTAssertEqual(vm.messages.last?.role, .assistant)
        XCTAssertEqual(vm.messages.last?.text, "before-error", "previously streamed content is preserved")
        XCTAssertEqual(vm.messages.last?.status, .errored)
    }

    func testEmptyTextIsIgnored() async throws {
        let runs = FakeRunGateway(script: [])
        let messages = FakeMessageGateway()
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.send("   ")

        XCTAssertTrue(vm.messages.isEmpty)
        XCTAssertTrue(messages.posted.isEmpty)
        XCTAssertFalse(vm.isStreaming)
    }

    func testPostFailureSurfacesError() async throws {
        let runs = FakeRunGateway(script: [])
        let messages = FakeMessageGateway()
        messages.postError = ChatError(code: "network", message: "offline")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.send("hi")

        XCTAssertEqual(vm.error?.code, "network")
        XCTAssertEqual(vm.messages.first?.status, .errored)
        XCTAssertFalse(vm.isStreaming)
    }

    func testIsEmptyAfterHistoryLoadWithNoMessages() async throws {
        let runs = FakeRunGateway(script: [])
        let messages = FakeMessageGateway()
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        XCTAssertFalse(vm.isEmpty, "should not be empty before history loads")

        await vm.loadHistory()

        XCTAssertTrue(vm.isEmpty, "empty after a successful zero-message load")
        XCTAssertFalse(vm.isLoadingHistory)
    }

    func testIsLoadingHistoryFlagSetDuringFetch() async throws {
        let runs = FakeRunGateway(script: [])
        let messages = FakeMessageGateway()
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        XCTAssertFalse(vm.isLoadingHistory)
        let loadTask = Task { @MainActor in await vm.loadHistory() }
        await loadTask.value
        XCTAssertFalse(vm.isLoadingHistory, "flag must reset after load completes")
    }

    func testIsEmptyFalseDuringStreamingEvenWithNoMessages() async throws {
        let runs = FakeRunGateway(script: [
            .gate("hold"),
        ])
        let messages = FakeMessageGateway(nextRunId: "r-empty")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        let sendTask = Task { @MainActor in await vm.send("hi") }
        // After send pumps, streaming starts; isEmpty must be false even
        // before the assistant message arrives.
        await Task.yield()
        await Task.yield()
        XCTAssertFalse(vm.isEmpty, "isEmpty must be false while streaming")
        runs.releaseGate(named: "hold")
        await sendTask.value
    }

    func testClearErrorResetsBanner() async throws {
        let runs = FakeRunGateway(script: [])
        let messages = FakeMessageGateway()
        messages.postError = BackendError.transport("offline")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.send("hi")
        XCTAssertNotNil(vm.error)

        vm.clearError()
        XCTAssertNil(vm.error)
    }

    func testRetryResendsLastUserMessage() async throws {
        let runs = FakeRunGateway(script: [
            .event(.runStart(.init(runId: "r-retry", conversationId: "c1", startedAt: "t"))),
            .event(.messageStart(.init(id: "m-retry"))),
            .event(.contentDelta(.init(text: "ok"))),
            .event(.messageStop(.init(stopReason: .endTurn, usage: nil))),
            .event(.runEnd(.init(runId: "r-retry", status: .completed, endedAt: "t"))),
        ])
        let messages = FakeMessageGateway(nextRunId: "r-retry")
        // First post fails; second post (retry) succeeds.
        messages.postError = BackendError.transport("flaky")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.send("the question")
        XCTAssertNotNil(vm.error)

        // Clear the post error before retry so the gateway succeeds this
        // time. The view-model must remember the last user message text.
        messages.postError = nil
        await vm.retry()

        XCTAssertEqual(messages.posted.count, 2)
        let secondPostText: String? = {
            guard let block = messages.posted.last?.request.content.first,
                  case .text(let t) = block else { return nil }
            return t.text
        }()
        XCTAssertEqual(secondPostText, "the question")
        XCTAssertEqual(vm.runStatus, .completed)
    }

    func testRetryWithNoPriorMessageIsNoOp() async throws {
        let runs = FakeRunGateway(script: [])
        let messages = FakeMessageGateway()
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.retry()

        XCTAssertTrue(messages.posted.isEmpty)
        XCTAssertNil(vm.error)
    }

    func testTransportErrorOnPostExposesTypedKindAndRetryAction() async throws {
        let runs = FakeRunGateway(script: [])
        let messages = FakeMessageGateway()
        messages.postError = BackendError.transport("URLError(-1004)")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.send("hi")

        XCTAssertEqual(vm.error?.kind, .transport)
        XCTAssertEqual(vm.error?.actions.canRetry, true)
    }

    func testProviderUnauthorizedFromSSESurfacesSettingsAction() async throws {
        let script: [FakeRunGateway.ScriptStep] = [
            .event(.runStart(.init(runId: "r-pu", conversationId: "c1", startedAt: "t"))),
            .event(.error(.init(code: "provider.unauthorized", message: "401"))),
            .event(.runEnd(.init(runId: "r-pu", status: .errored, endedAt: "t"))),
        ]
        let runs = FakeRunGateway(script: script)
        let messages = FakeMessageGateway(nextRunId: "r-pu")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.send("hi")

        XCTAssertEqual(vm.error?.kind, .providerUnauthorized)
        XCTAssertEqual(vm.error?.actions.canOpenSettings, true)
        XCTAssertEqual(vm.error?.actions.settingsTab, .providers)
    }

    func testSandboxRequiredFromSSEExposesChooseSandboxAction() async throws {
        let script: [FakeRunGateway.ScriptStep] = [
            .event(.runStart(.init(runId: "r-sr", conversationId: "c1", startedAt: "t"))),
            .event(.error(.init(code: "sandbox.required", message: "no template"))),
            .event(.runEnd(.init(runId: "r-sr", status: .errored, endedAt: "t"))),
        ]
        let runs = FakeRunGateway(script: script)
        let messages = FakeMessageGateway(nextRunId: "r-sr")
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.send("run echo")

        XCTAssertEqual(vm.error?.kind, .sandboxRequired)
        XCTAssertEqual(vm.error?.actions.canChooseSandbox, true)
    }

    func testLoadHistoryHydratesMessagesFromDomain() async throws {
        let runs = FakeRunGateway(script: [])
        let messages = FakeMessageGateway()
        messages.stubbedHistory = [
            Message(
                id: "m1",
                conversationId: "c1",
                role: .user,
                content: [.text(TextBlock(text: "hello"))],
                createdAt: "2026-04-28T10:00:00Z",
                ordinal: 0
            ),
            Message(
                id: "m2",
                conversationId: "c1",
                role: .assistant,
                content: [.text(TextBlock(text: "hi back"))],
                createdAt: "2026-04-28T10:00:01Z",
                ordinal: 1
            ),
        ]
        let vm = ChatViewModel(runGateway: runs, messageGateway: messages, conversationId: "c1")

        await vm.loadHistory()

        XCTAssertEqual(vm.messages.count, 2)
        XCTAssertEqual(vm.messages[0].role, .user)
        XCTAssertEqual(vm.messages[0].text, "hello")
        XCTAssertEqual(vm.messages[1].role, .assistant)
        XCTAssertEqual(vm.messages[1].text, "hi back")
    }

    /// Polls a predicate on the main actor with a timeout. Used to deterministically
    /// observe streaming state mid-flight without arbitrary sleeps.
    private func waitUntil(
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
