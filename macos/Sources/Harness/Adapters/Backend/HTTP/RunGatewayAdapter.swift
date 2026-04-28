import Foundation

public struct RunGatewayAdapter: RunGateway {
    private let client: HTTPClient
    private let sseReader: SSEReader

    public init(client: HTTPClient, sseReader: SSEReader? = nil) {
        self.client = client
        self.sseReader = sseReader ?? SSEReader(session: client.underlyingSession)
    }

    public func events(runId: String, lastEventId: String?) async throws -> AsyncThrowingStream<RunEvent, Error> {
        var headers: [String: String] = ["Accept": "text/event-stream"]
        if let lastEventId {
            headers["Last-Event-ID"] = lastEventId
        }
        let request = client.makeRequest(
            method: "GET",
            path: "/v1/runs/\(runId)/events",
            headers: headers
        )
        return sseReader.stream(request: request)
    }

    public func cancel(runId: String) async throws {
        let request = client.makeRequest(method: "POST", path: "/v1/runs/\(runId)/cancel")
        try await client.sendNoContent(request)
    }
}
