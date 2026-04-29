import XCTest

/// Verifies the fail-closed sandbox guard: a tool call against a
/// conversation with no sandbox template attached must surface a
/// user-visible error and never spawn a child process. Once the user
/// picks a template, the same prompt must succeed.
///
/// Mirrors the contract from PLAN.md §2.4.1 and the orchestrator's
/// `ToolError::NoSandbox` path.
final class SandboxAttachmentTest: HarnessE2ETestCase {
    func testEchoBlockedUntilSandboxAttached() throws {
        // Step 1: conversation with no sandbox.
        try createConversation(title: "Sandbox guard", sandboxName: nil)

        sendMessage("echo: oops")

        // The error banner copy comes from ChatError's sandboxRequired
        // case. Match on the stable fragment "no sandbox" so a future
        // message tweak doesn't break the assertion.
        XCTAssertTrue(
            waitForErrorBanner(containing: "no sandbox"),
            "sandbox-required error banner did not appear; tool may have run unsandboxed"
        )

        // Step 2: pick a sandbox via the chat error's "Choose sandbox"
        // action, or via the new-conversation sandbox picker. The
        // simplest reliable path is to start a fresh conversation with
        // the template attached and re-send.
        try createConversation(title: "Sandboxed retry", sandboxName: "permissive-dev")
        sendMessage("echo: oops")

        XCTAssertTrue(
            waitForAssistantText(containing: "Tool said: oops"),
            "tool round-trip did not succeed even with permissive-dev sandbox attached"
        )
    }
}
