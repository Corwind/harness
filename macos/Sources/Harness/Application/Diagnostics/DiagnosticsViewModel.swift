import Foundation

@Observable
@MainActor
public final class DiagnosticsViewModel {
    private let gateway: DiagnosticsGateway
    private let pollInterval: Duration
    private let maxLines: Int

    public private(set) var lines: [LogLine] = []
    public private(set) var nextSeq: Int = 0
    public private(set) var lastError: BackendError?
    public private(set) var isPolling: Bool = false

    private var pollTask: Task<Void, Never>?

    public init(
        gateway: DiagnosticsGateway,
        pollInterval: Duration = .seconds(1),
        maxLines: Int = 5_000
    ) {
        self.gateway = gateway
        self.pollInterval = pollInterval
        self.maxLines = maxLines
    }

    /// Begin polling. Idempotent — calling `start()` while already
    /// running is a no-op.
    public func start() async {
        if pollTask != nil { return }
        isPolling = true
        let task = Task { [weak self] in
            while !Task.isCancelled {
                await self?.pollOnce()
                let interval = await self?.pollInterval ?? .seconds(1)
                try? await Task.sleep(for: interval)
            }
        }
        pollTask = task
    }

    /// Stop polling. Idempotent.
    public func stop() async {
        pollTask?.cancel()
        pollTask = nil
        isPolling = false
    }

    /// Single poll iteration. Public so tests can drive deterministic
    /// transitions without relying on the timer; the running poll loop
    /// invokes the same path.
    public func pollOnce() async {
        let cursor: Int? = nextSeq > 0 ? nextSeq : nil
        do {
            let page = try await gateway.tail(afterSeq: cursor)
            append(page.logs)
            nextSeq = page.nextSeq
            lastError = nil
        } catch let backend as BackendError {
            lastError = backend
        } catch {
            lastError = .transport(String(describing: error))
        }
    }

    /// Reset the visible log buffer; the seq cursor is preserved so a
    /// subsequent poll only returns truly-new lines.
    public func clear() {
        lines.removeAll()
    }

    private func append(_ newLines: [LogLine]) {
        // Drop anything we already have (defensive — the server's `>`
        // filter normally guarantees this, but a future bug or a paused
        // ring shouldn't lead to UI duplicates).
        let known = Set(lines.map(\.seq))
        let fresh = newLines.filter { !known.contains($0.seq) }
        guard !fresh.isEmpty else { return }
        lines.append(contentsOf: fresh)
        // Keep ordered by seq.
        lines.sort(by: { $0.seq < $1.seq })
        // Cap retained count, evicting oldest first.
        if lines.count > maxLines {
            lines.removeFirst(lines.count - maxLines)
        }
    }
}
