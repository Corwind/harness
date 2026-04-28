import Foundation

public protocol SandboxTemplatesGateway: Sendable {
    func list() async throws -> [SandboxTemplate]
    func create(_ request: CreateSandboxTemplateRequest) async throws -> SandboxTemplate
    func get(id: String) async throws -> SandboxTemplate
    func patch(id: String, _ request: PatchSandboxTemplateRequest) async throws -> SandboxTemplate
    func delete(id: String) async throws
    func validate(id: String) async throws -> Bool
}
