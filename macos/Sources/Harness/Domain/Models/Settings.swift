import Foundation

public enum SettingsTheme: String, Codable, Sendable, Equatable {
    case light
    case dark
    case system
}

public struct Settings: Codable, Sendable, Equatable {
    public let theme: SettingsTheme?
    public let defaultProviderId: String?
    public let defaultModel: String?
    public let defaultSandboxTemplateId: String?
    public let requireSandboxForTools: Bool?

    public init(
        theme: SettingsTheme? = nil,
        defaultProviderId: String? = nil,
        defaultModel: String? = nil,
        defaultSandboxTemplateId: String? = nil,
        requireSandboxForTools: Bool? = nil
    ) {
        self.theme = theme
        self.defaultProviderId = defaultProviderId
        self.defaultModel = defaultModel
        self.defaultSandboxTemplateId = defaultSandboxTemplateId
        self.requireSandboxForTools = requireSandboxForTools
    }

    enum CodingKeys: String, CodingKey {
        case theme
        case defaultProviderId = "default_provider_id"
        case defaultModel = "default_model"
        case defaultSandboxTemplateId = "default_sandbox_template_id"
        case requireSandboxForTools = "require_sandbox_for_tools"
    }
}

public struct PatchSettingsRequest: Codable, Sendable, Equatable {
    public let theme: SettingsTheme?
    public let defaultProviderId: String?
    public let defaultModel: String?
    public let defaultSandboxTemplateId: String?
    public let requireSandboxForTools: Bool?

    public init(
        theme: SettingsTheme? = nil,
        defaultProviderId: String? = nil,
        defaultModel: String? = nil,
        defaultSandboxTemplateId: String? = nil,
        requireSandboxForTools: Bool? = nil
    ) {
        self.theme = theme
        self.defaultProviderId = defaultProviderId
        self.defaultModel = defaultModel
        self.defaultSandboxTemplateId = defaultSandboxTemplateId
        self.requireSandboxForTools = requireSandboxForTools
    }

    enum CodingKeys: String, CodingKey {
        case theme
        case defaultProviderId = "default_provider_id"
        case defaultModel = "default_model"
        case defaultSandboxTemplateId = "default_sandbox_template_id"
        case requireSandboxForTools = "require_sandbox_for_tools"
    }
}
