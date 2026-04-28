import XCTest
@testable import HarnessApp

@MainActor
final class SandboxAttachmentViewModelTests: XCTestCase {
    private static let templates: [SandboxTemplate] = [
        .fixture(id: "tpl_strict", name: "strict-readonly", isBuiltin: true),
        .fixture(id: "tpl_net", name: "no-network", isBuiltin: true),
        .fixture(id: "tpl_custom", name: "my-custom", isBuiltin: false),
    ]

    private func makeConversation(sandboxTemplateId: String?) -> Conversation {
        Conversation(
            id: "conv_1",
            title: "Test",
            providerId: "claude",
            model: "claude-3-5-sonnet",
            sandboxTemplateId: sandboxTemplateId,
            createdAt: "2026-04-28T00:00:00Z",
            updatedAt: "2026-04-28T00:00:00Z"
        )
    }

    // T2.5 #5 — picker integration: setting a new template patches the
    // conversation once with the new id.
    func testSetActiveTemplatePatchesConversationOnce() async {
        let conversation = makeConversation(sandboxTemplateId: nil)
        let conversations = FakeConversationGatewayForPicker(seeded: [conversation])
        let vm = SandboxAttachmentViewModel(
            conversationId: conversation.id,
            initialTemplateId: nil,
            availableTemplates: Self.templates,
            conversationGateway: conversations
        )

        await vm.setActiveTemplate(id: "tpl_strict")

        XCTAssertEqual(conversations.patchCallCount, 1)
        XCTAssertEqual(conversations.lastPatch?.id, "conv_1")
        XCTAssertEqual(
            conversations.lastPatch?.request.sandboxTemplateId?.value,
            "tpl_strict",
            "expected explicit non-null sandbox_template_id with id tpl_strict"
        )
        XCTAssertEqual(vm.activeTemplateId, "tpl_strict")
    }

    // T2.5 #5 — clearing sends an explicit-null patch.
    func testClearActiveTemplateSendsExplicitNullPatch() async {
        let conversation = makeConversation(sandboxTemplateId: "tpl_strict")
        let conversations = FakeConversationGatewayForPicker(seeded: [conversation])
        let vm = SandboxAttachmentViewModel(
            conversationId: conversation.id,
            initialTemplateId: "tpl_strict",
            availableTemplates: Self.templates,
            conversationGateway: conversations
        )

        await vm.clearActiveTemplate()

        XCTAssertEqual(conversations.patchCallCount, 1)
        // request.sandboxTemplateId is NullableField<String>?; lastPatch is itself
        // optional so we have a double-Optional to peel.
        guard let outerField = conversations.lastPatch?.request.sandboxTemplateId else {
            return XCTFail("sandboxTemplateId must be present (explicit-null patch, not omitted)")
        }
        XCTAssertNil(
            outerField.value,
            "expected explicit-null inner value to clear the conversation's sandbox"
        )
        XCTAssertNil(vm.activeTemplateId)
    }

    // T2.5 #6 — no-sandbox warning visible when no template is selected.
    func testNoSandboxWarningWhenTemplateIsNil() {
        let conversations = FakeConversationGatewayForPicker(seeded: [])
        let vm = SandboxAttachmentViewModel(
            conversationId: "conv_1",
            initialTemplateId: nil,
            availableTemplates: Self.templates,
            conversationGateway: conversations
        )
        XCTAssertTrue(vm.shouldShowNoSandboxWarning)
        XCTAssertEqual(
            vm.warningMessage,
            "Tools blocked — no sandbox attached"
        )
    }

    func testNoWarningWhenTemplateIsSet() {
        let conversations = FakeConversationGatewayForPicker(seeded: [])
        let vm = SandboxAttachmentViewModel(
            conversationId: "conv_1",
            initialTemplateId: "tpl_strict",
            availableTemplates: Self.templates,
            conversationGateway: conversations
        )
        XCTAssertFalse(vm.shouldShowNoSandboxWarning)
    }
}
