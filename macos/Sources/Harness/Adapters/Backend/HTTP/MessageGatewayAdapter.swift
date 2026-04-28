import Foundation

public struct MessageGatewayAdapter: MessageGateway {
    private let client: HTTPClient

    public init(client: HTTPClient) {
        self.client = client
    }

    private struct ListResponse: Decodable {
        let messages: [Message]
    }

    public func list(conversationId: String, limit: Int?, afterOrdinal: Int?) async throws -> [Message] {
        var query: [URLQueryItem] = []
        if let limit { query.append(URLQueryItem(name: "limit", value: String(limit))) }
        if let afterOrdinal {
            query.append(URLQueryItem(name: "after_ordinal", value: String(afterOrdinal)))
        }
        let request = client.makeRequest(
            method: "GET",
            path: "/v1/conversations/\(conversationId)/messages",
            query: query
        )
        return try await client.send(request, as: ListResponse.self).messages
    }

    public func post(conversationId: String, _ body: PostMessageRequest) async throws -> RunHandle {
        let data = try client.encode(body)
        let request = client.makeRequest(
            method: "POST",
            path: "/v1/conversations/\(conversationId)/messages",
            body: data
        )
        return try await client.send(request, as: RunHandle.self)
    }
}
