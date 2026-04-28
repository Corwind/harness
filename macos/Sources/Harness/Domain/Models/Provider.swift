import Foundation

public struct ProviderCapabilities: Codable, Sendable, Equatable {
    public let streaming: Bool
    public let tools: Bool
    public let vision: Bool
    public let systemPrompt: Bool
    public let maxContextTokens: Int?

    public init(
        streaming: Bool,
        tools: Bool,
        vision: Bool,
        systemPrompt: Bool,
        maxContextTokens: Int?
    ) {
        self.streaming = streaming
        self.tools = tools
        self.vision = vision
        self.systemPrompt = systemPrompt
        self.maxContextTokens = maxContextTokens
    }

    enum CodingKeys: String, CodingKey {
        case streaming
        case tools
        case vision
        case systemPrompt = "system_prompt"
        case maxContextTokens = "max_context_tokens"
    }
}

public struct Provider: Codable, Sendable, Equatable {
    public let id: String
    public let displayName: String
    public let configured: Bool
    public let capabilities: ProviderCapabilities

    public init(id: String, displayName: String, configured: Bool, capabilities: ProviderCapabilities) {
        self.id = id
        self.displayName = displayName
        self.configured = configured
        self.capabilities = capabilities
    }

    enum CodingKeys: String, CodingKey {
        case id
        case displayName = "display_name"
        case configured
        case capabilities
    }
}

public struct ProviderConfig: Codable, Sendable, Equatable {
    public let apiKey: String?
    public let baseURL: String?
    public let extra: JSONValue?

    public init(apiKey: String?, baseURL: String?, extra: JSONValue? = nil) {
        self.apiKey = apiKey
        self.baseURL = baseURL
        self.extra = extra
    }

    enum CodingKeys: String, CodingKey {
        case apiKey = "api_key"
        case baseURL = "base_url"
        case extra
    }
}

public struct ProviderConfigSummary: Codable, Sendable, Equatable {
    public let providerId: String
    public let configured: Bool
    public let updatedAt: String

    public init(providerId: String, configured: Bool, updatedAt: String) {
        self.providerId = providerId
        self.configured = configured
        self.updatedAt = updatedAt
    }

    enum CodingKeys: String, CodingKey {
        case providerId = "provider_id"
        case configured
        case updatedAt = "updated_at"
    }
}

public struct Model: Codable, Sendable, Equatable {
    public let id: String
    public let displayName: String
    public let contextWindow: Int?
    public let supportsTools: Bool?
    public let supportsVision: Bool?

    public init(
        id: String,
        displayName: String,
        contextWindow: Int? = nil,
        supportsTools: Bool? = nil,
        supportsVision: Bool? = nil
    ) {
        self.id = id
        self.displayName = displayName
        self.contextWindow = contextWindow
        self.supportsTools = supportsTools
        self.supportsVision = supportsVision
    }

    enum CodingKeys: String, CodingKey {
        case id
        case displayName = "display_name"
        case contextWindow = "context_window"
        case supportsTools = "supports_tools"
        case supportsVision = "supports_vision"
    }
}
