import Foundation

public struct ConversationsPage: Sendable, Equatable {
    public let conversations: [Conversation]
    public let nextCursor: String?

    public init(conversations: [Conversation], nextCursor: String? = nil) {
        self.conversations = conversations
        self.nextCursor = nextCursor
    }
}

public protocol ConversationGateway: Sendable {
    func list(limit: Int?, cursor: String?) async throws -> ConversationsPage
    func create(_ request: CreateConversationRequest) async throws -> Conversation
    func get(id: String) async throws -> Conversation
    func patch(id: String, _ request: PatchConversationRequest) async throws -> Conversation
    func delete(id: String) async throws
}
