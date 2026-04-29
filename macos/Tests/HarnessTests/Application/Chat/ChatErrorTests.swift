import XCTest
@testable import HarnessApp

/// Behavior coverage for the typed error → user-readable message mapping
/// that drives every chat-side error banner. Each case here represents a
/// failure path the chat surface must handle gracefully.
final class ChatErrorTests: XCTestCase {

    // MARK: - BackendError mapping

    func testTransportErrorMapsToOfflineMessageWithRetry() {
        let mapped = ChatError.from(BackendError.transport("URLError(-1004)"))
        XCTAssertEqual(mapped.kind, .transport)
        XCTAssertTrue(mapped.message.contains("offline"))
        XCTAssertTrue(mapped.actions.canRetry)
    }

    func testHttp401MapsToSessionExpiredFatal() {
        let mapped = ChatError.from(BackendError.httpStatus(401, body: nil))
        XCTAssertEqual(mapped.kind, .sessionExpired)
        XCTAssertFalse(mapped.actions.canRetry)
        XCTAssertFalse(mapped.actions.canOpenSettings)
        XCTAssertTrue(mapped.message.lowercased().contains("session"))
    }

    func testHttp403MapsToForbidden() {
        let mapped = ChatError.from(BackendError.httpStatus(403, body: nil))
        XCTAssertEqual(mapped.kind, .forbidden)
        XCTAssertFalse(mapped.actions.canRetry)
    }

    func testHttp500WithBodyDetailUsesDetailMessage() {
        let body = APIErrorBody(
            type: "about:blank", title: "internal", status: 500,
            detail: "the database is on fire"
        )
        let mapped = ChatError.from(BackendError.httpStatus(500, body: body))
        if case .server(let status, let detail) = mapped.kind {
            XCTAssertEqual(status, 500)
            XCTAssertEqual(detail, "the database is on fire")
        } else {
            XCTFail("expected .server kind, got \(mapped.kind)")
        }
        XCTAssertEqual(mapped.message, "the database is on fire")
    }

    func testHttpStatusWithProviderCodeRoutesThroughCodeClassifier() {
        // A REST endpoint can surface a typed provider error via body.code.
        let body = APIErrorBody(
            type: "about:blank", title: "unauthorized", status: 401,
            detail: "key rejected", code: "provider.unauthorized"
        )
        let mapped = ChatError.from(BackendError.httpStatus(401, body: body))
        XCTAssertEqual(mapped.kind, .providerUnauthorized)
        XCTAssertTrue(mapped.actions.canOpenSettings)
        XCTAssertEqual(mapped.actions.settingsTab, .providers)
    }

    // MARK: - SSE ErrorPayload mapping (server-emitted error events)

    func testProviderUnauthorizedShowsSettingsLink() {
        let mapped = ChatError.classify(
            code: "provider.unauthorized",
            message: "Anthropic returned 401."
        )
        XCTAssertEqual(mapped.kind, .providerUnauthorized)
        XCTAssertTrue(mapped.message.contains("API key"))
        XCTAssertTrue(mapped.actions.canOpenSettings)
        XCTAssertEqual(mapped.actions.settingsTab, .providers)
    }

    func testProviderRateLimitedExtractsRetryAfterFromMessage() {
        let mapped = ChatError.classify(
            code: "provider.rate_limited",
            message: "Anthropic asked us to retry-after 30s"
        )
        if case .providerRateLimited(let retry) = mapped.kind {
            XCTAssertEqual(retry, 30)
        } else {
            XCTFail("expected .providerRateLimited, got \(mapped.kind)")
        }
        XCTAssertTrue(mapped.actions.canRetry)
        XCTAssertTrue(mapped.message.contains("30s"))
    }

    func testProviderRateLimitedWithoutRetryAfterStillRetryable() {
        let mapped = ChatError.classify(
            code: "provider.rate_limited",
            message: "Slow down, partner."
        )
        if case .providerRateLimited(let retry) = mapped.kind {
            XCTAssertNil(retry)
        } else {
            XCTFail("expected .providerRateLimited")
        }
        XCTAssertTrue(mapped.actions.canRetry)
    }

    func testProviderUnconfiguredRoutesToSettingsProvidersTab() {
        let mapped = ChatError.classify(
            code: "provider.unconfigured",
            message: "no api_key on file"
        )
        XCTAssertEqual(mapped.kind, .providerUnconfigured)
        XCTAssertTrue(mapped.actions.canOpenSettings)
        XCTAssertEqual(mapped.actions.settingsTab, .providers)
    }

    func testProviderUnavailableIsRetryable() {
        let mapped = ChatError.classify(
            code: "provider.unavailable",
            message: "upstream timeout"
        )
        XCTAssertEqual(mapped.kind, .providerUnavailable)
        XCTAssertTrue(mapped.actions.canRetry)
    }

    func testProviderUpstreamSharesUnavailableKind() {
        let mapped = ChatError.classify(
            code: "provider.upstream",
            message: "upstream returned 503"
        )
        XCTAssertEqual(mapped.kind, .providerUnavailable)
        XCTAssertTrue(mapped.actions.canRetry)
    }

    func testSandboxRequiredOffersChooseSandboxAction() {
        let mapped = ChatError.classify(
            code: "sandbox.required",
            message: "Conversation has no sandbox template"
        )
        XCTAssertEqual(mapped.kind, .sandboxRequired)
        XCTAssertTrue(mapped.actions.canChooseSandbox)
        XCTAssertFalse(mapped.actions.canOpenSettings)
        XCTAssertTrue(mapped.message.contains("Tools blocked"))
    }

    func testSandboxInvalidProfileRoutesToSandboxesTab() {
        let mapped = ChatError.classify(
            code: "sandbox.invalid_profile",
            message: "(deny default) is malformed"
        )
        if case .sandboxInvalidProfile = mapped.kind {
            // expected
        } else {
            XCTFail("expected .sandboxInvalidProfile")
        }
        XCTAssertTrue(mapped.actions.canOpenSettings)
        if case .sandboxes = mapped.actions.settingsTab {
            // expected
        } else {
            XCTFail("expected sandboxes settings tab")
        }
    }

    func testToolTimeoutIsRetryable() {
        let mapped = ChatError.classify(code: "tool.timeout", message: "exceeded 30s")
        XCTAssertEqual(mapped.kind, .toolTimeout)
        XCTAssertTrue(mapped.actions.canRetry)
    }

    func testToolSpawnFailedIncludesUnderlyingMessage() {
        let mapped = ChatError.classify(
            code: "tool.spawn_failed",
            message: "/bin/echo: permission denied"
        )
        XCTAssertEqual(mapped.kind, .toolSpawnFailed)
        XCTAssertTrue(mapped.message.contains("permission denied"))
    }

    func testUnknownCodeFallsBackToOtherWithRawMessage() {
        let mapped = ChatError.classify(
            code: "something.unmapped",
            message: "a curious failure"
        )
        XCTAssertEqual(mapped.kind, .other)
        XCTAssertEqual(mapped.message, "a curious failure")
    }

    func testFromErrorPayloadDelegatesToCodeClassifier() {
        let payload = ErrorPayload(code: "provider.unauthorized", message: "key rejected")
        let mapped = ChatError.from(payload)
        XCTAssertEqual(mapped.kind, .providerUnauthorized)
    }
}
