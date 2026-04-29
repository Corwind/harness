import Foundation

public struct DiagnosticsGatewayAdapter: DiagnosticsGateway {
    private let client: HTTPClient

    public init(client: HTTPClient) {
        self.client = client
    }

    private struct Envelope: Decodable {
        let logs: [LogLine]
        let nextSeq: Int

        enum CodingKeys: String, CodingKey {
            case logs
            case nextSeq = "next_seq"
        }
    }

    public func tail(afterSeq: Int?) async throws -> LogPage {
        var query: [URLQueryItem] = []
        if let afterSeq, afterSeq > 0 {
            query.append(URLQueryItem(name: "after_seq", value: String(afterSeq)))
        }
        let request = client.makeRequest(
            method: "GET",
            path: "/v1/diagnostics/logs",
            query: query
        )
        let envelope = try await client.send(request, as: Envelope.self)
        return LogPage(logs: envelope.logs, nextSeq: envelope.nextSeq)
    }
}
