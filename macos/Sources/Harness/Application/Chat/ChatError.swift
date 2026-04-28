import Foundation

public struct ChatError: Sendable, Equatable, Error {
    public let code: String?
    public let message: String

    public init(code: String? = nil, message: String) {
        self.code = code
        self.message = message
    }

    public static func from(_ error: Error) -> ChatError {
        if let chatError = error as? ChatError { return chatError }
        return ChatError(code: nil, message: String(describing: error))
    }

    public static func from(_ payload: ErrorPayload) -> ChatError {
        ChatError(code: payload.code, message: payload.message)
    }
}
