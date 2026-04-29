import Foundation
@testable import HarnessApp

final class FakeSandboxTemplatesGateway: SandboxTemplatesGateway, @unchecked Sendable {
    private let lock = NSLock()
    private var templates: [SandboxTemplate]
    var validateResult: Result<ValidateSandboxResult, Error> = .success(.init(valid: true))

    /// When non-nil, `list()` returns this result instead of the in-memory
    /// `templates` (lets tests inject failures such as `BackendError.transport`).
    var listResult: Result<[SandboxTemplate], Error>?

    /// Optional async hook invoked at the start of `list()` before the
    /// result is produced. Tests use this to gate the call so they can
    /// observe the in-flight loading state.
    var beforeList: (@Sendable () async -> Void)?

    private(set) var listCallCount: Int = 0
    private(set) var createCallCount: Int = 0
    private(set) var lastCreate: CreateSandboxTemplateRequest?
    private(set) var deleteCallCount: Int = 0
    private(set) var deletedIds: [String] = []
    private(set) var validateCallCount: Int = 0
    private(set) var lastValidatedId: String?
    private(set) var patchCallCount: Int = 0
    private(set) var lastPatch: (id: String, body: PatchSandboxTemplateRequest)?

    init(seeded: [SandboxTemplate]) {
        self.templates = seeded
    }

    func list() async throws -> [SandboxTemplate] {
        let hook = lockedRead { self.beforeList }
        await hook?()
        return try lockedRead {
            self.listCallCount += 1
            if let listResult = self.listResult {
                return listResult
            }
            return .success(self.templates)
        }.get()
    }

    private func lockedRead<T>(_ block: () -> T) -> T {
        lock.lock(); defer { lock.unlock() }
        return block()
    }

    func create(_ request: CreateSandboxTemplateRequest) async throws -> SandboxTemplate {
        lock.lock(); defer { lock.unlock() }
        createCallCount += 1
        lastCreate = request
        let template = SandboxTemplate(
            id: "tpl_\(UUID().uuidString.prefix(8).lowercased())",
            name: request.name,
            description: request.description,
            profile: request.profile,
            isBuiltin: false,
            createdAt: "2026-04-28T00:00:00Z",
            updatedAt: "2026-04-28T00:00:00Z"
        )
        templates.append(template)
        return template
    }

    func get(id: String) async throws -> SandboxTemplate {
        lock.lock(); defer { lock.unlock() }
        guard let t = templates.first(where: { $0.id == id }) else {
            throw BackendError.httpStatus(404, body: nil)
        }
        return t
    }

    func patch(id: String, _ request: PatchSandboxTemplateRequest) async throws -> SandboxTemplate {
        lock.lock(); defer { lock.unlock() }
        patchCallCount += 1
        lastPatch = (id, request)
        guard let idx = templates.firstIndex(where: { $0.id == id }) else {
            throw BackendError.httpStatus(404, body: nil)
        }
        let existing = templates[idx]
        let updated = SandboxTemplate(
            id: existing.id,
            name: request.name ?? existing.name,
            description: request.description ?? existing.description,
            profile: request.profile ?? existing.profile,
            isBuiltin: existing.isBuiltin,
            createdAt: existing.createdAt,
            updatedAt: "2026-04-28T00:00:00Z"
        )
        templates[idx] = updated
        return updated
    }

    func delete(id: String) async throws {
        lock.lock(); defer { lock.unlock() }
        deleteCallCount += 1
        deletedIds.append(id)
        templates.removeAll(where: { $0.id == id })
    }

    func validate(id: String) async throws -> ValidateSandboxResult {
        lock.lock(); defer { lock.unlock() }
        validateCallCount += 1
        lastValidatedId = id
        return try validateResult.get()
    }
}

final class FakeConversationGatewayForPicker: ConversationGateway, @unchecked Sendable {
    struct PatchCall: Equatable {
        let id: String
        let request: PatchSandboxTemplateRequest? // unused
    }

    private let lock = NSLock()
    private var seeded: [Conversation]
    private(set) var patchCallCount: Int = 0
    private(set) var lastPatch: (id: String, request: PatchConversationRequest)?

    init(seeded: [Conversation] = []) {
        self.seeded = seeded
    }

    func list(limit: Int?, cursor: String?) async throws -> ConversationsPage {
        ConversationsPage(conversations: seeded, nextCursor: nil)
    }

    func create(_ request: CreateConversationRequest) async throws -> Conversation {
        throw BackendError.httpStatus(500, body: nil)
    }

    func get(id: String) async throws -> Conversation {
        lock.lock(); defer { lock.unlock() }
        guard let c = seeded.first(where: { $0.id == id }) else {
            throw BackendError.httpStatus(404, body: nil)
        }
        return c
    }

    func patch(id: String, _ request: PatchConversationRequest) async throws -> Conversation {
        lock.lock(); defer { lock.unlock() }
        patchCallCount += 1
        lastPatch = (id, request)
        guard let idx = seeded.firstIndex(where: { $0.id == id }) else {
            throw BackendError.httpStatus(404, body: nil)
        }
        let existing = seeded[idx]
        let newSandboxId: String?
        if let field = request.sandboxTemplateId {
            newSandboxId = field.value
        } else {
            newSandboxId = existing.sandboxTemplateId
        }
        let updated = Conversation(
            id: existing.id,
            title: request.title ?? existing.title,
            providerId: existing.providerId,
            model: request.model ?? existing.model,
            sandboxTemplateId: newSandboxId,
            createdAt: existing.createdAt,
            updatedAt: "2026-04-28T00:00:00Z"
        )
        seeded[idx] = updated
        return updated
    }

    func delete(id: String) async throws {
        lock.lock(); defer { lock.unlock() }
        seeded.removeAll(where: { $0.id == id })
    }
}

extension SandboxTemplate {
    static func fixture(
        id: String,
        name: String,
        isBuiltin: Bool = false,
        profile: String = "(version 1) (deny default)"
    ) -> SandboxTemplate {
        SandboxTemplate(
            id: id,
            name: name,
            description: nil,
            profile: profile,
            isBuiltin: isBuiltin,
            createdAt: "2026-04-28T00:00:00Z",
            updatedAt: "2026-04-28T00:00:00Z"
        )
    }
}
