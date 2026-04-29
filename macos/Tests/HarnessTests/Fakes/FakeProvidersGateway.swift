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

    /// Optional async hook invoked at the start of `list()` before the
    /// result is produced. Tests use this to gate the call so they can
    /// observe in-flight loading states.
    var beforeList: (@Sendable () async -> Void)?
    var beforeListModels: (@Sendable (String) async -> Void)?

    private(set) var listCallCount: Int = 0
    private(set) var upsertCallCount: Int = 0
    private(set) var lastUpsert: UpsertCall?
    private(set) var listModelsCallCount: Int = 0
    private(set) var lastListModelsProviderId: String?

    func list() async throws -> [Provider] {
        let hook = lockedRead { self.beforeList }
        await hook?()
        return try lockedRead {
            self.listCallCount += 1
            return self.listResult
        }.get()
    }

    func upsertConfig(providerId: String, _ config: ProviderConfig) async throws -> ProviderConfigSummary {
        return try lockedRead {
            self.upsertCallCount += 1
            self.lastUpsert = UpsertCall(providerId: providerId, config: config)
            if let result = self.upsertResult {
                return result
            }
            return .success(ProviderConfigSummary(
                providerId: providerId,
                configured: true,
                updatedAt: "2026-04-28T00:00:00Z"
            ))
        }.get()
    }

    func listModels(providerId: String) async throws -> [Model] {
        let hook = lockedRead { self.beforeListModels }
        await hook?(providerId)
        return try lockedRead {
            self.listModelsCallCount += 1
            self.lastListModelsProviderId = providerId
            return self.modelsResult
        }.get()
    }

    private func lockedRead<T>(_ block: () -> T) -> T {
        lock.lock(); defer { lock.unlock() }
        return block()
    }
}
