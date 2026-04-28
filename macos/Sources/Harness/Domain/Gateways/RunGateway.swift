import Foundation

public protocol RunGateway: Sendable {
    /// Subscribe to the SSE event stream for a run. Cancelling iteration of the
    /// returned `AsyncThrowingStream` cancels the underlying HTTP request.
    func events(runId: String, lastEventId: String?) async throws -> AsyncThrowingStream<RunEvent, Error>

    func cancel(runId: String) async throws
}
