import XCTest

/// Headline e2e: drive a real Harness.app through a streaming Claude
/// conversation against the deterministic fake provider.
///
/// The fake's behaviour is documented in
/// `backend/crates/harness-server/src/testing.rs`:
///   * vanilla user turn → "Hello from fake provider."
///   * `"echo: <payload>"` → echo tool round-trip → assistant says
///     `"Tool said: <payload>."`
///
/// These tests exercise the full path: SwiftUI → HTTPClient → axum →
/// orchestrator → fake provider → SSE → ChatViewModel → SwiftUI.
final class EndToEndChatTests: HarnessE2ETestCase {
    func testChatRootRendersAfterLaunch() throws {
        // setUp() already waited for the chat root to render. This test
        // documents the minimum-viable post-bootstrap state and is the
        // canary that everything below assumes.
        XCTAssertTrue(
            app.buttons["New Conversation"].exists,
            "sidebar New Conversation button must be present after launch"
        )
    }

    func testSendingHelloYieldsFakeGreeting() throws {
        try createConversation(title: "Hello smoke")

        sendMessage("hello")

        XCTAssertTrue(
            waitForAssistantText(containing: "Hello from fake provider"),
            "fake provider greeting did not appear in the chat"
        )
    }

    func testEchoTriggersToolRoundTrip() throws {
        // The echo path needs a sandbox attached, otherwise the
        // orchestrator fails closed. permissive-dev is one of the
        // built-in templates seeded on first run.
        try createConversation(title: "Echo round-trip", sandboxName: "permissive-dev")

        sendMessage("echo: world")

        // The orchestrator runs the echo tool, feeds the result back,
        // and the fake's second turn quotes the output as
        // "Tool said: world.". The intermediate tool-use block also
        // renders, but we assert only the terminal text — that's the
        // observable contract callers care about.
        XCTAssertTrue(
            waitForAssistantText(containing: "Tool said: world"),
            "tool round-trip output did not appear; the echo tool may not have been invoked"
        )
    }
}
