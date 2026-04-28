import Foundation
@testable import HarnessApp

final class FakeProvidersGateway: ProvidersGateway, @unchecked Sendable {
    struct UpsertCall: Equatable {
        let providerId: String
        let config: ProviderConfig
    }

    private let lock = NSLock()
    var listResult: Result<[Provider], Error> = .success([])
    var upsertResult: Result<ProviderConfigSummary, Error>?
    var modelsResult: Result<[Model], Error> = .success([])

    private(set) var listCallCount: Int = 0
    private(set) var upsertCallCount: Int = 0
    private(set) var lastUpsert: UpsertCall?
    private(set) var listModelsCallCount: Int = 0
    private(set) var lastListModelsProviderId: String?

    func list() async throws -> [Provider] {
        lock.lock(); defer { lock.unlock() }
        listCallCount += 1
        return try listResult.get()
    }

    func upsertConfig(providerId: String, _ config: ProviderConfig) async throws -> ProviderConfigSummary {
        lock.lock(); defer { lock.unlock() }
        upsertCallCount += 1
        lastUpsert = UpsertCall(providerId: providerId, config: config)
        if let result = upsertResult {
            return try result.get()
        }
        return ProviderConfigSummary(
            providerId: providerId,
            configured: true,
            updatedAt: "2026-04-28T00:00:00Z"
        )
    }

    func listModels(providerId: String) async throws -> [Model] {
        lock.lock(); defer { lock.unlock() }
        listModelsCallCount += 1
        lastListModelsProviderId = providerId
        return try modelsResult.get()
    }
}
