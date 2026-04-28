import Foundation

public struct SettingsGatewayAdapter: SettingsGateway {
    private let client: HTTPClient

    public init(client: HTTPClient) {
        self.client = client
    }

    public func get() async throws -> Settings {
        let request = client.makeRequest(method: "GET", path: "/v1/settings")
        return try await client.send(request, as: Settings.self)
    }

    public func patch(_ body: PatchSettingsRequest) async throws -> Settings {
        let data = try client.encode(body)
        let request = client.makeRequest(method: "PATCH", path: "/v1/settings", body: data)
        return try await client.send(request, as: Settings.self)
    }
}
