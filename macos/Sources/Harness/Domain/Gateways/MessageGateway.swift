import Foundation

public protocol MessageGateway: Sendable {
    func list(conversationId: String, limit: Int?, afterOrdinal: Int?) async throws -> [Message]
    func post(conversationId: String, _ request: PostMessageRequest) async throws -> RunHandle
}
