import Foundation

public struct SandboxTemplate: Codable, Sendable, Equatable {
    public let id: String
    public let name: String
    public let description: String?
    public let profile: String
    public let isBuiltin: Bool
    public let createdAt: String
    public let updatedAt: String

    public init(
        id: String,
        name: String,
        description: String?,
        profile: String,
        isBuiltin: Bool,
        createdAt: String,
        updatedAt: String
    ) {
        self.id = id
        self.name = name
        self.description = description
        self.profile = profile
        self.isBuiltin = isBuiltin
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case description
        case profile
        case isBuiltin = "is_builtin"
        case createdAt = "created_at"
        case updatedAt = "updated_at"
    }
}

public struct CreateSandboxTemplateRequest: Codable, Sendable, Equatable {
    public let name: String
    public let description: String?
    public let profile: String

    public init(name: String, description: String? = nil, profile: String) {
        self.name = name
        self.description = description
        self.profile = profile
    }
}

public struct PatchSandboxTemplateRequest: Codable, Sendable, Equatable {
    public let name: String?
    public let description: String?
    public let profile: String?

    public init(name: String? = nil, description: String? = nil, profile: String? = nil) {
        self.name = name
        self.description = description
        self.profile = profile
    }
}
