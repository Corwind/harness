import Foundation

public struct SandboxTemplatesGatewayAdapter: SandboxTemplatesGateway {
    private let client: HTTPClient

    public init(client: HTTPClient) {
        self.client = client
    }

    private struct ListResponse: Decodable {
        let templates: [SandboxTemplate]
    }

    private struct ValidateResponse: Decodable {
        let valid: Bool
    }

    public func list() async throws -> [SandboxTemplate] {
        let request = client.makeRequest(method: "GET", path: "/v1/sandbox-templates")
        return try await client.send(request, as: ListResponse.self).templates
    }

    public func create(_ body: CreateSandboxTemplateRequest) async throws -> SandboxTemplate {
        let data = try client.encode(body)
        let request = client.makeRequest(method: "POST", path: "/v1/sandbox-templates", body: data)
        return try await client.send(request, as: SandboxTemplate.self)
    }

    public func get(id: String) async throws -> SandboxTemplate {
        let request = client.makeRequest(method: "GET", path: "/v1/sandbox-templates/\(id)")
        return try await client.send(request, as: SandboxTemplate.self)
    }

    public func patch(id: String, _ body: PatchSandboxTemplateRequest) async throws -> SandboxTemplate {
        let data = try client.encode(body)
        let request = client.makeRequest(method: "PATCH", path: "/v1/sandbox-templates/\(id)", body: data)
        return try await client.send(request, as: SandboxTemplate.self)
    }

    public func delete(id: String) async throws {
        let request = client.makeRequest(method: "DELETE", path: "/v1/sandbox-templates/\(id)")
        try await client.sendNoContent(request)
    }

    public func validate(id: String) async throws -> Bool {
        let request = client.makeRequest(method: "POST", path: "/v1/sandbox-templates/\(id)/validate")
        return try await client.send(request, as: ValidateResponse.self).valid
    }
}
