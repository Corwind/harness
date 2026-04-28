import Foundation

public protocol ProvidersGateway: Sendable {
    func list() async throws -> [Provider]
    func listModels(providerId: String) async throws -> [Model]
    func upsertConfig(providerId: String, _ config: ProviderConfig) async throws -> ProviderConfigSummary
}
