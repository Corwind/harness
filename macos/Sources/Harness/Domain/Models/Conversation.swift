import Foundation

public struct Conversation: Codable, Sendable, Equatable {
    public let id: String
    public let title: String
    public let providerId: String
    public let model: String
    public let sandboxTemplateId: String?
    public let createdAt: String
    public let updatedAt: String

    public init(
        id: String,
        title: String,
        providerId: String,
        model: String,
        sandboxTemplateId: String?,
        createdAt: String,
        updatedAt: String
    ) {
        self.id = id
        self.title = title
        self.providerId = providerId
        self.model = model
        self.sandboxTemplateId = sandboxTemplateId
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    enum CodingKeys: String, CodingKey {
        case id
        case title
        case providerId = "provider_id"
        case model
        case sandboxTemplateId = "sandbox_template_id"
        case createdAt = "created_at"
        case updatedAt = "updated_at"
    }
}

public struct CreateConversationRequest: Codable, Sendable, Equatable {
    public let providerId: String
    public let model: String
    public let title: String?
    public let sandboxTemplateId: String?

    public init(providerId: String, model: String, title: String? = nil, sandboxTemplateId: String? = nil) {
        self.providerId = providerId
        self.model = model
        self.title = title
        self.sandboxTemplateId = sandboxTemplateId
    }

    enum CodingKeys: String, CodingKey {
        case providerId = "provider_id"
        case model
        case title
        case sandboxTemplateId = "sandbox_template_id"
    }
}

public struct PatchConversationRequest: Codable, Sendable, Equatable {
    public let title: String?
    public let model: String?
    /// Use `.some(nil)` to clear the sandbox; `.none` to leave unchanged.
    public let sandboxTemplateId: NullableField<String>?

    public init(
        title: String? = nil,
        model: String? = nil,
        sandboxTemplateId: NullableField<String>? = nil
    ) {
        self.title = title
        self.model = model
        self.sandboxTemplateId = sandboxTemplateId
    }

    enum CodingKeys: String, CodingKey {
        case title
        case model
        case sandboxTemplateId = "sandbox_template_id"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.title = try c.decodeIfPresent(String.self, forKey: .title)
        self.model = try c.decodeIfPresent(String.self, forKey: .model)
        if c.contains(.sandboxTemplateId) {
            if try c.decodeNil(forKey: .sandboxTemplateId) {
                self.sandboxTemplateId = .null
            } else {
                let v = try c.decode(String.self, forKey: .sandboxTemplateId)
                self.sandboxTemplateId = .value(v)
            }
        } else {
            self.sandboxTemplateId = nil
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encodeIfPresent(title, forKey: .title)
        try c.encodeIfPresent(model, forKey: .model)
        if let s = sandboxTemplateId {
            if let v = s.value {
                try c.encode(v, forKey: .sandboxTemplateId)
            } else {
                try c.encodeNil(forKey: .sandboxTemplateId)
            }
        }
    }
}

/// Helper to distinguish "absent" (the surrounding optional is `nil`) from
/// "explicit null" (the wrapper is present and `value` is `nil`).
public struct NullableField<Wrapped: Codable & Sendable & Equatable>: Codable, Sendable, Equatable {
    public let value: Wrapped?

    public init(_ value: Wrapped?) { self.value = value }

    public static func value(_ v: Wrapped) -> NullableField<Wrapped> { .init(v) }
    public static var null: NullableField<Wrapped> { .init(nil) }

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() {
            self.value = nil
        } else {
            self.value = try c.decode(Wrapped.self)
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        if let v = value {
            try c.encode(v)
        } else {
            try c.encodeNil()
        }
    }
}
