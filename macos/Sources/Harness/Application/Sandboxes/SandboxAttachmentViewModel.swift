import Foundation

/// View model that backs `SandboxPickerView` when it's used to drive the
/// sandbox attachment of a *specific* conversation. The picker stays a
/// dumb-renderer; this type owns the conversation id, the available
/// template list, and the gateway call.
@Observable
@MainActor
public final class SandboxAttachmentViewModel {
    public let conversationId: String
    public let availableTemplates: [SandboxTemplate]
    public private(set) var activeTemplateId: String?
    public private(set) var attachError: String?
    public private(set) var isUpdating: Bool = false

    private let conversationGateway: ConversationGateway

    public init(
        conversationId: String,
        initialTemplateId: String?,
        availableTemplates: [SandboxTemplate],
        conversationGateway: ConversationGateway
    ) {
        self.conversationId = conversationId
        self.activeTemplateId = initialTemplateId
        self.availableTemplates = availableTemplates
        self.conversationGateway = conversationGateway
    }

    public var activeTemplate: SandboxTemplate? {
        guard let id = activeTemplateId else { return nil }
        return availableTemplates.first(where: { $0.id == id })
    }

    public var shouldShowNoSandboxWarning: Bool { activeTemplateId == nil }
    public var warningMessage: String { "Tools blocked — no sandbox attached" }

    public func setActiveTemplate(id: String) async {
        await applyPatch(.value(id)) { [weak self] in
            self?.activeTemplateId = id
        }
    }

    public func clearActiveTemplate() async {
        await applyPatch(.null) { [weak self] in
            self?.activeTemplateId = nil
        }
    }

    private func applyPatch(
        _ field: NullableField<String>,
        onSuccess: @MainActor () -> Void
    ) async {
        attachError = nil
        isUpdating = true
        defer { isUpdating = false }
        do {
            _ = try await conversationGateway.patch(
                id: conversationId,
                PatchConversationRequest(sandboxTemplateId: field)
            )
            onSuccess()
        } catch {
            attachError = String(describing: error)
        }
    }
}
