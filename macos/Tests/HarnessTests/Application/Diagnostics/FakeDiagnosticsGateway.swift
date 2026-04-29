import Foundation
@testable import HarnessApp

/// Scriptable fake for `DiagnosticsGateway`. Tests configure a sequence
/// of `tail(afterSeq:)` results; the `i`-th call resolves to the `i`-th
/// result. Once the script is exhausted, further calls return an empty
/// page that echoes the cursor.
final class FakeDiagnosticsGateway: DiagnosticsGateway, @unchecked Sendable {
    private let lock = NSLock()
    private var script: [Result<LogPage, Error>]
    private(set) var calls: [Int?] = []

    /// Optional async hook fired before the script result is produced.
    /// Tests use this to synchronise with the polling loop.
    var beforeTail: (@Sendable (Int?) async -> Void)?

    init(script: [Result<LogPage, Error>]) {
        self.script = script
    }

    func tail(afterSeq: Int?) async throws -> LogPage {
        let hook = lockedRead { self.beforeTail }
        await hook?(afterSeq)
        let next: Result<LogPage, Error>? = lockedRead {
            self.calls.append(afterSeq)
            if self.script.isEmpty {
                return nil
            }
            return self.script.removeFirst()
        }
        if let next {
            return try next.get()
        }
        return LogPage(logs: [], nextSeq: afterSeq ?? 0)
    }

    private func lockedRead<T>(_ block: () -> T) -> T {
        lock.lock(); defer { lock.unlock() }
        return block()
    }
}

extension LogLine {
    static func fixture(
        seq: Int,
        level: LogLevel = .info,
        target: String = "harness_server",
        message: String = "ok",
        ts: String = "2026-04-29T08:00:00Z"
    ) -> LogLine {
        LogLine(seq: seq, level: level, ts: ts, target: target, message: message)
    }
}
