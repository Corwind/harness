import Foundation

public enum MessageRole: String, Codable, Sendable, Equatable {
    case user
    case assistant
    case tool
    case system
}

public struct TextBlock: Codable, Sendable, Equatable {
    public let text: String

    public init(text: String) { self.text = text }
}

public struct ToolUseBlock: Codable, Sendable, Equatable {
    public let id: String
    public let name: String
    public let input: [String: JSONValue]

    public init(id: String, name: String, input: [String: JSONValue]) {
        self.id = id
        self.name = name
        self.input = input
    }
}

public struct ToolResultBlock: Codable, Sendable, Equatable {
    public let toolUseId: String
    public let output: JSONValue
    public let isError: Bool

    public init(toolUseId: String, output: JSONValue, isError: Bool = false) {
        self.toolUseId = toolUseId
        self.output = output
        self.isError = isError
    }

    enum CodingKeys: String, CodingKey {
        case toolUseId = "tool_use_id"
        case output
        case isError = "is_error"
    }
}

public enum ImageSource: Codable, Sendable, Equatable {
    case base64(mediaType: String, data: String)
    case url(String)

    enum CodingKeys: String, CodingKey {
        case kind, mediaType = "media_type", data, url
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let kind = try c.decode(String.self, forKey: .kind)
        switch kind {
        case "base64":
            let mt = try c.decode(String.self, forKey: .mediaType)
            let data = try c.decode(String.self, forKey: .data)
            self = .base64(mediaType: mt, data: data)
        case "url":
            let url = try c.decode(String.self, forKey: .url)
            self = .url(url)
        default:
            throw DecodingError.dataCorruptedError(
                forKey: .kind, in: c, debugDescription: "Unknown image source kind: \(kind)"
            )
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .base64(let mt, let data):
            try c.encode("base64", forKey: .kind)
            try c.encode(mt, forKey: .mediaType)
            try c.encode(data, forKey: .data)
        case .url(let url):
            try c.encode("url", forKey: .kind)
            try c.encode(url, forKey: .url)
        }
    }
}

public struct ImageBlock: Codable, Sendable, Equatable {
    public let source: ImageSource

    public init(source: ImageSource) { self.source = source }
}

public enum MessageContentBlock: Codable, Sendable, Equatable {
    case text(TextBlock)
    case toolUse(ToolUseBlock)
    case toolResult(ToolResultBlock)
    case image(ImageBlock)

    private enum DiscriminatorKey: String, CodingKey { case type }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: DiscriminatorKey.self)
        let type = try c.decode(String.self, forKey: .type)
        switch type {
        case "text":
            self = .text(try TextBlock(from: decoder))
        case "tool_use":
            self = .toolUse(try ToolUseBlock(from: decoder))
        case "tool_result":
            self = .toolResult(try ToolResultBlock(from: decoder))
        case "image":
            self = .image(try ImageBlock(from: decoder))
        default:
            throw DecodingError.dataCorruptedError(
                forKey: .type, in: c, debugDescription: "Unknown content block type: \(type)"
            )
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: DiscriminatorKey.self)
        switch self {
        case .text(let b):
            try c.encode("text", forKey: .type)
            try b.encode(to: encoder)
        case .toolUse(let b):
            try c.encode("tool_use", forKey: .type)
            try b.encode(to: encoder)
        case .toolResult(let b):
            try c.encode("tool_result", forKey: .type)
            try b.encode(to: encoder)
        case .image(let b):
            try c.encode("image", forKey: .type)
            try b.encode(to: encoder)
        }
    }
}

public struct Message: Codable, Sendable, Equatable {
    public let id: String
    public let conversationId: String
    public let role: MessageRole
    public let content: [MessageContentBlock]
    public let createdAt: String
    public let ordinal: Int

    public init(
        id: String,
        conversationId: String,
        role: MessageRole,
        content: [MessageContentBlock],
        createdAt: String,
        ordinal: Int
    ) {
        self.id = id
        self.conversationId = conversationId
        self.role = role
        self.content = content
        self.createdAt = createdAt
        self.ordinal = ordinal
    }

    enum CodingKeys: String, CodingKey {
        case id
        case conversationId = "conversation_id"
        case role
        case content
        case createdAt = "created_at"
        case ordinal
    }
}

public struct PostMessageRequest: Codable, Sendable, Equatable {
    public let content: [MessageContentBlock]
    public let maxTokens: Int?
    public let temperature: Double?

    public init(content: [MessageContentBlock], maxTokens: Int? = nil, temperature: Double? = nil) {
        self.content = content
        self.maxTokens = maxTokens
        self.temperature = temperature
    }

    enum CodingKeys: String, CodingKey {
        case content
        case maxTokens = "max_tokens"
        case temperature
    }
}

public struct RunHandle: Codable, Sendable, Equatable {
    public let runId: String
    public let conversationId: String
    public let messageId: String

    public init(runId: String, conversationId: String, messageId: String) {
        self.runId = runId
        self.conversationId = conversationId
        self.messageId = messageId
    }

    enum CodingKeys: String, CodingKey {
        case runId = "run_id"
        case conversationId = "conversation_id"
        case messageId = "message_id"
    }
}
