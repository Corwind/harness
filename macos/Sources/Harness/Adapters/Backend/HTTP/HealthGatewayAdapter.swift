import Foundation

public struct HealthGatewayAdapter: HealthGateway {
    private let client: HTTPClient

    public init(client: HTTPClient) {
        self.client = client
    }

    public func getHealth() async throws -> Health {
        let request = client.makeRequest(method: "GET", path: "/v1/health")
        return try await client.send(request, as: Health.self)
    }
}
