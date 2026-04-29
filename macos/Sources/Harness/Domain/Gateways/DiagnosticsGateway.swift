import Foundation

/// Reads recent backend log lines for the Diagnostics tab in Settings.
/// `afterSeq == nil` performs the initial poll (returns whatever the ring
/// is holding); subsequent calls pass the previous page's `nextSeq` so
/// only new lines come back.
public protocol DiagnosticsGateway: Sendable {
    func tail(afterSeq: Int?) async throws -> LogPage
}
