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

/// Result of a `POST /v1/sandbox-templates/{id}/validate` call.
///
/// The server returns `200 {valid: true}` on success and `200 {valid: false,
/// stderr: "..."}` when the profile is malformed (per the T1.L brief — the
/// UI needs the diagnostic to render inline). HTTP-level errors map to
/// thrown gateway errors.
public struct ValidateSandboxResult: Codable, Sendable, Equatable {
    public let valid: Bool
    public let stderr: String?

    public init(valid: Bool, stderr: String? = nil) {
        self.valid = valid
        self.stderr = stderr
    }
}
