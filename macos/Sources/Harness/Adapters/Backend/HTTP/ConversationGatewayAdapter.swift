import Foundation

public struct ConversationGatewayAdapter: ConversationGateway {
    private let client: HTTPClient

    public init(client: HTTPClient) {
        self.client = client
    }

    private struct ListResponse: Decodable {
        let conversations: [Conversation]
        let nextCursor: String?
        enum CodingKeys: String, CodingKey {
            case conversations
            case nextCursor = "next_cursor"
        }
    }

    public func list(limit: Int?, cursor: String?) async throws -> ConversationsPage {
        var query: [URLQueryItem] = []
        if let limit { query.append(URLQueryItem(name: "limit", value: String(limit))) }
        if let cursor { query.append(URLQueryItem(name: "cursor", value: cursor)) }
        let request = client.makeRequest(method: "GET", path: "/v1/conversations", query: query)
        let resp = try await client.send(request, as: ListResponse.self)
        return ConversationsPage(conversations: resp.conversations, nextCursor: resp.nextCursor)
    }

    public func create(_ body: CreateConversationRequest) async throws -> Conversation {
        let data = try client.encode(body)
        let request = client.makeRequest(method: "POST", path: "/v1/conversations", body: data)
        return try await client.send(request, as: Conversation.self)
    }

    public func get(id: String) async throws -> Conversation {
        let request = client.makeRequest(method: "GET", path: "/v1/conversations/\(id)")
        return try await client.send(request, as: Conversation.self)
    }

    public func patch(id: String, _ body: PatchConversationRequest) async throws -> Conversation {
        let data = try client.encode(body)
        let request = client.makeRequest(method: "PATCH", path: "/v1/conversations/\(id)", body: data)
        return try await client.send(request, as: Conversation.self)
    }

    public func delete(id: String) async throws {
        let request = client.makeRequest(method: "DELETE", path: "/v1/conversations/\(id)")
        try await client.sendNoContent(request)
    }
}
