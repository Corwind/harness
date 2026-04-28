import XCTest
@testable import HarnessApp

@MainActor
final class SettingsViewModelTests: XCTestCase {
    // T1.I behavior 3 — theme picker writes through:
    // Selecting "Dark" via the view model updates the currentTheme and invokes
    // SettingsGateway.patch(theme:) exactly once with `.dark`.
    func testSetThemeWritesThroughToGateway() async {
        let gateway = FakeSettingsGateway(initial: Settings(theme: .light))
        let providers = FakeProvidersGateway()
        let vm = SettingsViewModel(settings: gateway, providers: providers)
        await vm.load()

        await vm.setTheme(.dark)

        XCTAssertEqual(gateway.patchCallCount, 1)
        XCTAssertEqual(gateway.lastPatch?.theme, .dark)
        XCTAssertEqual(vm.appTheme, .dark)
    }

    // T1.I behavior 4 — provider config form validates:
    // - Empty API key → typed validation error; gateway not called.
    // - Non-empty key → ProvidersGateway.upsertConfig(...) called once with the expected payload.
    func testUpsertProviderRejectsEmptyApiKey() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        await vm.upsertProvider(id: "claude", apiKey: "", baseUrl: nil)

        XCTAssertEqual(providers.upsertCallCount, 0)
        XCTAssertEqual(vm.providerError, .emptyApiKey)
    }

    func testUpsertProviderCallsGatewayOnceWithExpectedPayload() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        await vm.upsertProvider(id: "claude", apiKey: "sk-test", baseUrl: "https://api.example.com")

        XCTAssertEqual(providers.upsertCallCount, 1)
        XCTAssertEqual(providers.lastUpsert?.providerId, "claude")
        XCTAssertEqual(providers.lastUpsert?.config.apiKey, "sk-test")
        XCTAssertEqual(providers.lastUpsert?.config.baseURL, "https://api.example.com")
        XCTAssertNil(vm.providerError)
    }

    // T1.I behavior 5 — bad key surfaces user-readable error:
    // Fake gateway returns `BackendError.httpStatus(401, _)` (== unauthorized) →
    // SettingsViewModel exposes a non-empty user-readable error string.
    func testUnauthorizedFromGatewaySurfacesAsReadableError() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        providers.upsertResult = .failure(BackendError.httpStatus(401, body: nil))
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        await vm.upsertProvider(id: "claude", apiKey: "sk-bad", baseUrl: nil)

        XCTAssertEqual(vm.providerError, .unauthorized)
        XCTAssertFalse(vm.providerErrorMessage.isEmpty)
        XCTAssertTrue(
            vm.providerErrorMessage.lowercased().contains("api key")
                || vm.providerErrorMessage.lowercased().contains("unauthorized")
                || vm.providerErrorMessage.lowercased().contains("rejected"),
            "Unauthorized message should mention the API key / unauthorized state — got '\(vm.providerErrorMessage)'"
        )
    }

    // T1.I behavior 6 — model list refresh:
    // Configured provider → ProvidersGateway.listModels(...) called once →
    // resulting [Model] is exposed on the view model.
    func testRefreshModelsExposesGatewayResult() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        let claude = Model(id: "claude-3-5-sonnet", displayName: "Claude 3.5 Sonnet", contextWindow: 200_000)
        let haiku = Model(id: "claude-3-5-haiku", displayName: "Claude 3.5 Haiku", contextWindow: 200_000)
        providers.modelsResult = .success([claude, haiku])
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        await vm.refreshModels(providerId: "claude")

        XCTAssertEqual(providers.listModelsCallCount, 1)
        XCTAssertEqual(providers.lastListModelsProviderId, "claude")
        XCTAssertEqual(vm.models["claude"]?.map(\.id), [claude.id, haiku.id])
    }
}
