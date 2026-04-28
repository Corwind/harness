import XCTest
@testable import HarnessApp

/// End-to-end behavior tests for `SettingsViewModel` against the real
/// `harness-server` binary. Tests that need a fake Claude provider live
/// in `SettingsViewModelHTTPTests` (URLProtocol-mocked) since the
/// production binary's Claude HTTP client cannot deterministically
/// reproduce all error paths in CI.
@MainActor
final class SettingsViewModelE2ETests: XCTestCase {
    private var harness: LiveBackendHarness?

    override func tearDown() {
        harness?.shutdown()
        harness = nil
        super.tearDown()
    }

    private func boot() async throws -> (LiveBackendHarness, SettingsGatewayAdapter, ProvidersGatewayAdapter) {
        let h: LiveBackendHarness
        do {
            h = try await LiveBackendHarness()
        } catch LiveBackendHarness.Error.binaryUnavailable {
            throw XCTSkip("harness-server binary not available — build it with `make build`")
        }
        self.harness = h
        let session = URLSession(configuration: .ephemeral)
        let client = HTTPClient(
            baseURL: h.session.baseURL,
            token: h.session.token,
            session: session
        )
        return (h, SettingsGatewayAdapter(client: client), ProvidersGatewayAdapter(client: client))
    }

    /// T2.2 #1 — provider config round-trip against a real backend.
    /// Spawn `harness-server`, upsertProvider("claude", apiKey:"test"),
    /// then GET /v1/providers and assert the provider is `configured`.
    func testProviderConfigRoundTripPersistsAndIsListedAsConfigured() async throws {
        let (_, settings, providers) = try await boot()
        let vm = SettingsViewModel(settings: settings, providers: providers)

        // Sanity: providers list reflects a registered-but-unconfigured Claude.
        await vm.refreshProviders()
        XCTAssertNil(vm.settingsLoadError)
        let claudeBefore = vm.providersList.first(where: { $0.id == "claude" })
        XCTAssertNotNil(claudeBefore, "claude provider must be registered")
        XCTAssertEqual(claudeBefore?.configured, false)

        await vm.upsertProvider(id: "claude", apiKey: "test", baseUrl: nil)
        XCTAssertNil(vm.providerError, "expected no error; got \(String(describing: vm.providerError))")

        // The view model refreshes the providers list after upsert; assert it
        // reflects the new configured state, and double-check via the raw GET.
        let claudeAfter = vm.providersList.first(where: { $0.id == "claude" })
        XCTAssertEqual(claudeAfter?.configured, true)

        let raw = try await providers.list()
        XCTAssertEqual(raw.first(where: { $0.id == "claude" })?.configured, true)
    }

    /// T2.2 #4 — settings KV round-trip.
    /// `setTheme(.dark)` writes through the gateway; a fresh view model
    /// loading from the same backend reflects the dark setting.
    func testSetThemeWritesThroughAndReloadsAsDark() async throws {
        let (_, settings, providers) = try await boot()
        let vm = SettingsViewModel(settings: settings, providers: providers)

        await vm.setTheme(.dark)
        XCTAssertEqual(vm.appTheme, .dark)
        XCTAssertNil(vm.settingsLoadError)

        // Spin up a *second* view model bound to the same backend and load.
        // It must see the persisted dark setting.
        let vm2 = SettingsViewModel(settings: settings, providers: providers)
        await vm2.load()
        XCTAssertEqual(vm2.appTheme, .dark)
        XCTAssertEqual(vm2.currentTheme.background, Theme.dark.background)
    }
}
