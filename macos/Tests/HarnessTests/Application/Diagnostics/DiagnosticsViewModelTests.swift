import XCTest
@testable import HarnessApp

@MainActor
final class DiagnosticsViewModelTests: XCTestCase {
    // T3.5b #1 — polling appends only new lines (no duplicates, ordered by seq).
    func testPollAppendsOnlyNewLines() async {
        let gateway = FakeDiagnosticsGateway(script: [
            .success(LogPage(logs: [
                .fixture(seq: 1, message: "first"),
                .fixture(seq: 2, message: "second"),
            ], nextSeq: 2)),
            .success(LogPage(logs: [
                .fixture(seq: 3, message: "third"),
            ], nextSeq: 3)),
            .success(LogPage(logs: [], nextSeq: 3)), // empty poll keeps cursor stable
        ])
        let vm = DiagnosticsViewModel(gateway: gateway, pollInterval: .milliseconds(10))

        await vm.pollOnce()
        XCTAssertEqual(vm.lines.map(\.seq), [1, 2])
        XCTAssertEqual(vm.nextSeq, 2)

        await vm.pollOnce()
        XCTAssertEqual(vm.lines.map(\.seq), [1, 2, 3])
        XCTAssertEqual(vm.nextSeq, 3)

        await vm.pollOnce()
        XCTAssertEqual(vm.lines.map(\.seq), [1, 2, 3], "empty poll must not append duplicates")
        XCTAssertEqual(vm.nextSeq, 3)
    }

    func testPollSendsAfterSeqCursor() async {
        let gateway = FakeDiagnosticsGateway(script: [
            .success(LogPage(logs: [.fixture(seq: 5, message: "a")], nextSeq: 5)),
            .success(LogPage(logs: [.fixture(seq: 6, message: "b")], nextSeq: 6)),
        ])
        let vm = DiagnosticsViewModel(gateway: gateway, pollInterval: .milliseconds(10))

        await vm.pollOnce()
        await vm.pollOnce()

        // First call: nil afterSeq (initial poll); second call: 5 (the
        // largest seq returned).
        XCTAssertEqual(gateway.calls, [nil, 5])
    }

    // T3.5b #2 — backend offline → lastError set; resumes when reachable.
    func testBackendOfflineSetsLastErrorAndResumes() async {
        let gateway = FakeDiagnosticsGateway(script: [
            .failure(BackendError.transport("connection refused")),
            .success(LogPage(logs: [.fixture(seq: 1, message: "back")], nextSeq: 1)),
        ])
        let vm = DiagnosticsViewModel(gateway: gateway, pollInterval: .milliseconds(10))

        await vm.pollOnce()
        XCTAssertEqual(vm.lastError, BackendError.transport("connection refused"))
        XCTAssertTrue(vm.lines.isEmpty)

        await vm.pollOnce()
        XCTAssertNil(vm.lastError, "successful poll must clear the error")
        XCTAssertEqual(vm.lines.map(\.seq), [1])
    }

    // T3.5b #3 — start()/stop() lifecycle drives multiple polls.
    func testStartStopLifecycle() async {
        let gateway = FakeDiagnosticsGateway(script: [
            .success(LogPage(logs: [.fixture(seq: 1)], nextSeq: 1)),
            .success(LogPage(logs: [.fixture(seq: 2)], nextSeq: 2)),
            .success(LogPage(logs: [.fixture(seq: 3)], nextSeq: 3)),
        ])
        let vm = DiagnosticsViewModel(gateway: gateway, pollInterval: .milliseconds(20))

        await vm.start()

        // Poll until we've observed at least 2 lines or the timeout fires.
        let deadline = Date().addingTimeInterval(2.0)
        while vm.lines.count < 2 && Date() < deadline {
            try? await Task.sleep(nanoseconds: 5_000_000)
        }
        await vm.stop()

        XCTAssertGreaterThanOrEqual(vm.lines.count, 2,
                                    "polling task must have appended at least 2 lines before stop")
        // Stopping must prevent further appends.
        let snapshot = vm.lines.count
        try? await Task.sleep(nanoseconds: 100_000_000)
        XCTAssertEqual(vm.lines.count, snapshot, "stop() must halt the polling task")
    }

    func testCappingDropsOldestOnceMaxLinesExceeded() async {
        // Configure a small cap and feed a stream that exceeds it.
        let gateway = FakeDiagnosticsGateway(script: [
            .success(LogPage(logs: (1...5).map { .fixture(seq: $0) }, nextSeq: 5)),
            .success(LogPage(logs: (6...10).map { .fixture(seq: $0) }, nextSeq: 10)),
        ])
        let vm = DiagnosticsViewModel(gateway: gateway, pollInterval: .milliseconds(10), maxLines: 6)

        await vm.pollOnce()
        await vm.pollOnce()

        XCTAssertEqual(vm.lines.count, 6, "view model must cap its retained line count")
        XCTAssertEqual(vm.lines.first?.seq, 5)
        XCTAssertEqual(vm.lines.last?.seq, 10)
    }
}
