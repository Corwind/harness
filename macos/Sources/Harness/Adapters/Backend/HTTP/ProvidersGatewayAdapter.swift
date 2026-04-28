import Foundation

public struct ProvidersGatewayAdapter: ProvidersGateway {
    private let client: HTTPClient

    public init(client: HTTPClient) {
        self.client = client
    }

    private struct ListResponse: Decodable {
        let providers: [Provider]
    }

    private struct ModelsResponse: Decodable {
        let models: [Model]
    }

    public func list() async throws -> [Provider] {
        let request = client.makeRequest(method: "GET", path: "/v1/providers")
        return try await client.send(request, as: ListResponse.self).providers
    }

    public func listModels(providerId: String) async throws -> [Model] {
        let request = client.makeRequest(method: "GET", path: "/v1/providers/\(providerId)/models")
        return try await client.send(request, as: ModelsResponse.self).models
    }

    public func upsertConfig(providerId: String, _ config: ProviderConfig) async throws -> ProviderConfigSummary {
        let data = try client.encode(config)
        let request = client.makeRequest(
            method: "POST",
            path: "/v1/providers/\(providerId)/config",
            body: data
        )
        return try await client.send(request, as: ProviderConfigSummary.self)
    }
}
