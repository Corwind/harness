import XCTest
@testable import HarnessApp

final class SSEReaderTests: XCTestCase {

    private var router: MockHTTPProtocol.Router!
    private var session: URLSession!

    override func setUp() {
        super.setUp()
        router = MockHTTPProtocol.Router()
        MockHTTPProtocol.install(router: router)
        session = MockHTTPProtocol.makeSession()
    }

    override func tearDown() {
        MockHTTPProtocol.reset()
        session = nil
        router = nil
        super.tearDown()
    }

    private func makeAdapter(token: String = "tok") -> RunGatewayAdapter {
        let client = HTTPClient(
            baseURL: URL(string: "http://127.0.0.1:8080")!,
            token: token,
            session: session
        )
        let reader = SSEReader(session: session)
        return RunGatewayAdapter(client: client, sseReader: reader)
    }

    // 3. SSE: fixture run.start → message.start → 3× content.delta → message.stop → run.end
    func test_sse_textOnlyRun_parsesEventsInOrder() async throws {
        let stream = """
        event: run.start
        id: 1
        data: {"run_id":"run_1","conversation_id":"conv_1","started_at":"2026-04-28T10:15:00Z"}

        event: message.start
        id: 2
        data: {"id":"msg_1"}

        event: content.delta
        id: 3
        data: {"text":"Hel"}

        event: content.delta
        id: 4
        data: {"text":"lo, "}

        event: content.delta
        id: 5
        data: {"text":"world"}

        event: message.stop
        id: 6
        data: {"stop_reason":"end_turn","usage":{"input_tokens":12,"output_tokens":8}}

        event: run.end
        id: 7
        data: {"run_id":"run_1","status":"completed","ended_at":"2026-04-28T10:15:01Z"}


        """
        router.register { _ in
            .init(
                status: 200,
                headers: ["Content-Type": "text/event-stream"],
                body: stream.data(using: .utf8)!
            )
        }

        let adapter = makeAdapter()
        let events = try await collect(stream: adapter.events(runId: "run_1", lastEventId: nil))

        XCTAssertEqual(events.count, 7)
        guard case .runStart(let rs) = events[0] else { return XCTFail("expected run.start") }
        XCTAssertEqual(rs.runId, "run_1")
        XCTAssertEqual(rs.conversationId, "conv_1")

        guard case .messageStart(let ms) = events[1] else { return XCTFail("expected message.start") }
        XCTAssertEqual(ms.id, "msg_1")

        let deltaTexts = events[2...4].compactMap { event -> String? in
            if case .contentDelta(let p) = event { return p.text } else { return nil }
        }
        XCTAssertEqual(deltaTexts, ["Hel", "lo, ", "world"])

        guard case .messageStop(let stop) = events[5] else { return XCTFail("expected message.stop") }
        XCTAssertEqual(stop.stopReason, .endTurn)
        XCTAssertEqual(stop.usage?.inputTokens, 12)
        XCTAssertEqual(stop.usage?.outputTokens, 8)

        guard case .runEnd(let end) = events[6] else { return XCTFail("expected run.end") }
        XCTAssertEqual(end.status, .completed)
    }

    // 4. SSE fixture: tool_use.start → tool_use.delta → tool_use.stop → tool.start →
    //                tool.stdout → tool.finish → message.stop → run.end
    func test_sse_toolUseRun_parsesEventsInOrder() async throws {
        let stream = """
        event: run.start
        id: 1
        data: {"run_id":"run_2","conversation_id":"conv_2","started_at":"2026-04-28T10:15:00Z"}

        event: message.start
        id: 2
        data: {"id":"msg_2"}

        event: tool_use.start
        id: 3
        data: {"id":"toolu_1","name":"read_file"}

        event: tool_use.delta
        id: 4
        data: {"id":"toolu_1","partial_json":"{\\"path\\":\\"/et"}

        event: tool_use.stop
        id: 5
        data: {"id":"toolu_1","input":{"path":"/etc/hosts"}}

        event: tool.start
        id: 6
        data: {"tool_use_id":"toolu_1","name":"read_file","kind":"external","sandbox_template_id":"tpl_strict_readonly"}

        event: tool.stdout
        id: 7
        data: {"tool_use_id":"toolu_1","chunk":"127.0.0.1\\tlocalhost\\n"}

        event: tool.finish
        id: 8
        data: {"tool_use_id":"toolu_1","output":{"stdout":"127.0.0.1\\tlocalhost\\n","stderr":"","exit_code":0},"exit_code":0,"duration_ms":12}

        event: message.stop
        id: 9
        data: {"stop_reason":"tool_use"}

        event: run.end
        id: 10
        data: {"run_id":"run_2","status":"completed","ended_at":"2026-04-28T10:15:04Z"}


        """
        router.register { _ in
            .init(
                status: 200,
                headers: ["Content-Type": "text/event-stream"],
                body: stream.data(using: .utf8)!
            )
        }

        let adapter = makeAdapter()
        let events = try await collect(stream: adapter.events(runId: "run_2", lastEventId: nil))

        XCTAssertEqual(events.count, 10)

        guard case .toolUseStart(let tus) = events[2] else { return XCTFail("expected tool_use.start") }
        XCTAssertEqual(tus.id, "toolu_1")
        XCTAssertEqual(tus.name, "read_file")

        guard case .toolUseDelta(let tud) = events[3] else { return XCTFail("expected tool_use.delta") }
        XCTAssertEqual(tud.id, "toolu_1")
        XCTAssertEqual(tud.partialJSON, #"{"path":"/et"#)

        guard case .toolUseStop(let tup) = events[4] else { return XCTFail("expected tool_use.stop") }
        XCTAssertEqual(tup.id, "toolu_1")
        XCTAssertEqual(tup.input["path"], .string("/etc/hosts"))

        guard case .toolStart(let ts) = events[5] else { return XCTFail("expected tool.start") }
        XCTAssertEqual(ts.toolUseId, "toolu_1")
        XCTAssertEqual(ts.kind, .external)
        XCTAssertEqual(ts.sandboxTemplateId, "tpl_strict_readonly")

        guard case .toolStdout(let tout) = events[6] else { return XCTFail("expected tool.stdout") }
        XCTAssertEqual(tout.toolUseId, "toolu_1")
        XCTAssertTrue(tout.chunk.contains("localhost"))

        guard case .toolFinish(let fin) = events[7] else { return XCTFail("expected tool.finish") }
        XCTAssertEqual(fin.toolUseId, "toolu_1")
        XCTAssertEqual(fin.exitCode, 0)
        XCTAssertEqual(fin.durationMs, 12)

        guard case .messageStop(let stop) = events[8] else { return XCTFail("expected message.stop") }
        XCTAssertEqual(stop.stopReason, .toolUse)

        guard case .runEnd = events[9] else { return XCTFail("expected run.end") }
    }

    // 5. SSE cancellation: cancelling the AsyncSequence iteration closes the
    //    URLSessionDataTask. The mock server records cancellation when the
    //    streaming task is cancelled mid-stream.
    func test_sse_cancellation_closesUnderlyingDataTask() async throws {
        // Build many chunks each with a small delay so we have time to cancel mid-stream.
        var chunks: [Data] = []
        let runStart = """
        event: run.start
        id: 1
        data: {"run_id":"run_3","conversation_id":"conv_3","started_at":"2026-04-28T10:15:00Z"}

        event: message.start
        id: 2
        data: {"id":"msg_3"}

        """.data(using: .utf8)!
        chunks.append(runStart)
        for i in 0..<50 {
            let frame = """
            event: content.delta
            id: \(i + 3)
            data: {"text":"chunk-\(i)"}

            """.data(using: .utf8)!
            chunks.append(frame)
        }
        router.register { _ in
            .init(
                status: 200,
                headers: ["Content-Type": "text/event-stream"],
                bodyChunks: chunks,
                delayBetweenChunks: 0.05
            )
        }

        let adapter = makeAdapter()
        let stream = try await adapter.events(runId: "run_3", lastEventId: nil)

        var receivedFirstDelta = false
        let consumer = Task {
            do {
                for try await event in stream {
                    if case .contentDelta = event {
                        receivedFirstDelta = true
                        break
                    }
                }
            } catch {
                // Cancellation may surface as BackendError.cancelled — accept it.
            }
        }
        // Wait for first delta to arrive then cancel the sequence iteration via Task.
        let waitDeadline = Date().addingTimeInterval(5)
        while !receivedFirstDelta && Date() < waitDeadline {
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        consumer.cancel()
        _ = await consumer.value

        // Allow MockHTTPProtocol to observe URLProtocol.stopLoading (the canonical
        // signal that URLSession cancelled the data task).
        let cancelDeadline = Date().addingTimeInterval(2)
        while router.stoppedPaths.isEmpty && Date() < cancelDeadline {
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTAssertTrue(router.stoppedPaths.contains("/v1/runs/run_3/events"),
                      "expected URLSessionDataTask to be cancelled mid-stream, got stops=\(router.stoppedPaths) cancels=\(router.cancelledPaths)")
    }

    // Auth header is injected on the SSE request as well.
    func test_sse_requestCarriesAuthHeader() async throws {
        let body = """
        event: run.end
        id: 1
        data: {"run_id":"r","status":"completed","ended_at":"2026-04-28T10:15:04Z"}


        """.data(using: .utf8)!
        router.register { _ in
            .init(status: 200, headers: ["Content-Type": "text/event-stream"], body: body)
        }
        let adapter = makeAdapter(token: "sse-tok")
        let events = try await collect(stream: adapter.events(runId: "r", lastEventId: "evt-5"))
        XCTAssertEqual(events.count, 1)
        XCTAssertEqual(router.observed.count, 1)
        XCTAssertEqual(router.observed[0].value(forHTTPHeaderField: "X-Harness-Token"), "sse-tok")
        XCTAssertEqual(router.observed[0].value(forHTTPHeaderField: "Last-Event-ID"), "evt-5")
        XCTAssertEqual(router.observed[0].value(forHTTPHeaderField: "Accept"), "text/event-stream")
    }

    // Helper: drain an AsyncThrowingStream<RunEvent, Error> into an array.
    private func collect(stream: AsyncThrowingStream<RunEvent, Error>) async throws -> [RunEvent] {
        var out: [RunEvent] = []
        for try await e in stream { out.append(e) }
        return out
    }
}
