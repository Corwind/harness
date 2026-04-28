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
        XCTAssertEqual(vm.error?.message, "429")
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
