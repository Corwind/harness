import Foundation
@testable import HarnessApp

final class FakeSettingsGateway: SettingsGateway, @unchecked Sendable {
    private let lock = NSLock()
    private var stored: Settings
    private(set) var patchCallCount: Int = 0
    private(set) var lastPatch: PatchSettingsRequest?
    private(set) var getCallCount: Int = 0

    init(initial: Settings) {
        self.stored = initial
    }

    func get() async throws -> Settings {
        lock.lock(); defer { lock.unlock() }
        getCallCount += 1
        return stored
    }

    func patch(_ request: PatchSettingsRequest) async throws -> Settings {
        lock.lock(); defer { lock.unlock() }
        patchCallCount += 1
        lastPatch = request
        stored = Settings(
            theme: request.theme ?? stored.theme,
            defaultProviderId: request.defaultProviderId ?? stored.defaultProviderId,
            defaultModel: request.defaultModel ?? stored.defaultModel,
            defaultSandboxTemplateId: request.defaultSandboxTemplateId ?? stored.defaultSandboxTemplateId,
            requireSandboxForTools: request.requireSandboxForTools ?? stored.requireSandboxForTools
        )
        return stored
    }
}
