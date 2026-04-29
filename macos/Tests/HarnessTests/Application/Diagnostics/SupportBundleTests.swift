import XCTest
@testable import HarnessApp

final class SupportBundleTests: XCTestCase {
    // T3.5b #3 — export bundle redacts API keys.
    // Any value of any provider's `api_key` field becomes "***".
    func testApiKeyValuesAreRedactedInExportedBundle() {
        let logs: [LogLine] = [
            .fixture(seq: 1, message: "starting up"),
            .fixture(seq: 2, level: .warn, message: "deprecation notice"),
        ]
        let configs: [SupportBundle.ProviderSnapshot] = [
            .init(
                providerId: "claude",
                configured: true,
                apiKey: "sk-secret-1234",
                baseURL: "https://api.anthropic.com"
            ),
            .init(
                providerId: "gemini",
                configured: false,
                apiKey: nil,
                baseURL: nil
            ),
        ]

        let bundle = SupportBundle.build(
            logs: logs,
            providers: configs,
            generatedAt: "2026-04-29T08:00:00Z",
            appVersion: "0.1.0",
            backendURL: "http://127.0.0.1:51823"
        )

        XCTAssertFalse(
            bundle.contains("sk-secret-1234"),
            "exported bundle must never contain the raw API key"
        )
        XCTAssertTrue(
            bundle.contains("api_key: \"***\""),
            "configured provider's api_key must render as \"***\""
        )
        // Make sure non-secret fields and log lines are still present.
        XCTAssertTrue(bundle.contains("claude"))
        XCTAssertTrue(bundle.contains("https://api.anthropic.com"))
        XCTAssertTrue(bundle.contains("starting up"))
        XCTAssertTrue(bundle.contains("deprecation notice"))
        XCTAssertTrue(bundle.contains("WARN"))
        XCTAssertTrue(bundle.contains("0.1.0"))
        XCTAssertTrue(bundle.contains("127.0.0.1:51823"))
    }

    func testUnconfiguredProviderRendersWithoutApiKeyLine() {
        let configs: [SupportBundle.ProviderSnapshot] = [
            .init(providerId: "ollama", configured: false, apiKey: nil, baseURL: nil),
        ]
        let bundle = SupportBundle.build(
            logs: [],
            providers: configs,
            generatedAt: "2026-04-29T08:00:00Z",
            appVersion: "0.1.0",
            backendURL: "http://127.0.0.1:51823"
        )
        XCTAssertTrue(bundle.contains("ollama"))
        XCTAssertFalse(bundle.contains("api_key:"),
                       "unconfigured provider must not emit an api_key line at all")
    }

    func testRedactionAppliesEvenIfApiKeyIsEmptyString() {
        // Defence in depth: even an empty-string apiKey must still go
        // through the redaction path (which renders "***") rather than
        // leaking the actual stored value.
        let configs: [SupportBundle.ProviderSnapshot] = [
            .init(providerId: "claude", configured: true, apiKey: "", baseURL: nil),
        ]
        let bundle = SupportBundle.build(
            logs: [],
            providers: configs,
            generatedAt: "2026-04-29T08:00:00Z",
            appVersion: "0.1.0",
            backendURL: "http://127.0.0.1:51823"
        )
        XCTAssertTrue(bundle.contains("api_key: \"***\""))
    }
}
