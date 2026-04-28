import Foundation
@testable import HarnessApp

/// A fake `RunGateway` that emits a scripted sequence of `RunEvent`s on the
/// stream returned by `events`. The script can include yielded events and a
/// terminating result (finish or throw). A pause point can be inserted with
/// `.gate` so tests can deterministically interleave actions like cancel().
public final class FakeRunGateway: RunGateway, @unchecked Sendable {
    public enum ScriptStep: Sendable {
        case event(RunEvent)
        case error(ChatError)
        /// Suspend until the test signals via `releaseGate(named:)`.
        case gate(String)
    }

    private let lock = NSLock()
    private var script: [ScriptStep]
    private var gates: [String: CheckedContinuation<Void, Never>] = [:]
    private var pendingReleases: Set<String> = []

    public private(set) var cancelCalls: [String] = []
    public private(set) var eventsRequests: [(runId: String, lastEventId: String?)] = []

    public var onCancel: (@Sendable (String) -> Void)?

    public init(script: [ScriptStep]) {
        self.script = script
    }

    public func events(runId: String, lastEventId: String?) async throws
        -> AsyncThrowingStream<RunEvent, Error>
    {
        lock.lock()
        eventsRequests.append((runId, lastEventId))
        let steps = script
        lock.unlock()

        return AsyncThrowingStream { continuation in
            let task = Task { [weak self] in
                for step in steps {
                    if Task.isCancelled { break }
                    switch step {
                    case .event(let e):
                        continuation.yield(e)
                    case .error(let err):
                        continuation.finish(throwing: err)
                        return
                    case .gate(let name):
                        await self?.waitForGate(name)
                        if Task.isCancelled { break }
                    }
                }
                continuation.finish()
            }
            continuation.onTermination = { _ in
                task.cancel()
            }
        }
    }

    public func cancel(runId: String) async throws {
        lock.lock()
        cancelCalls.append(runId)
        let cb = onCancel
        lock.unlock()
        cb?(runId)
    }

    public func releaseGate(named name: String) {
        lock.lock()
        if let cont = gates.removeValue(forKey: name) {
            lock.unlock()
            cont.resume()
        } else {
            pendingReleases.insert(name)
            lock.unlock()
        }
    }

    private func waitForGate(_ name: String) async {
        await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
            lock.lock()
            if pendingReleases.remove(name) != nil {
                lock.unlock()
                cont.resume()
            } else {
                gates[name] = cont
                lock.unlock()
            }
        }
    }
}

public final class FakeMessageGateway: MessageGateway, @unchecked Sendable {
    private let lock = NSLock()
    private var nextRunId: String
    public private(set) var posted: [(conversationId: String, request: PostMessageRequest)] = []
    public private(set) var listed: [(conversationId: String, limit: Int?, afterOrdinal: Int?)] = []
    public var stubbedHistory: [Message] = []
    public var postError: Error?

    public init(nextRunId: String = "run_test_01") {
        self.nextRunId = nextRunId
    }

    public func list(conversationId: String, limit: Int?, afterOrdinal: Int?) async throws -> [Message] {
        lock.lock()
        listed.append((conversationId, limit, afterOrdinal))
        let history = stubbedHistory
        lock.unlock()
        return history
    }

    public func post(conversationId: String, _ request: PostMessageRequest) async throws -> RunHandle {
        lock.lock()
        posted.append((conversationId, request))
        let runId = nextRunId
        let err = postError
        lock.unlock()
        if let err { throw err }
        return RunHandle(runId: runId, conversationId: conversationId, messageId: "msg-\(runId)")
    }
}
