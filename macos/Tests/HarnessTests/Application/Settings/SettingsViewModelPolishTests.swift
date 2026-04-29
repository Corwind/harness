import XCTest
@testable import HarnessApp

@MainActor
final class SettingsViewModelPolishTests: XCTestCase {
    // T3.1b #1 — typed errors produce user-readable strings.

    func testUpsertProviderUnauthorizedSurfacesInlineFormError() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        providers.upsertResult = .failure(BackendError.httpStatus(
            401,
            body: APIErrorBody(
                type: "about:blank",
                title: "unauthorized",
                status: 401,
                detail: "api key rejected",
                code: "provider.unauthorized"
            )
        ))
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        await vm.upsertProvider(id: "claude", apiKey: "sk-bad", baseUrl: nil)

        XCTAssertEqual(vm.providerError, .unauthorized)
        XCTAssertEqual(vm.providerErrorMessage, "Key rejected by provider")
    }

    func testRefreshModelsUnauthorizedExposesTypedError() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        providers.modelsResult = .failure(BackendError.httpStatus(
            401,
            body: APIErrorBody(type: "about:blank", title: "unauthorized", status: 401, code: "provider.unauthorized")
        ))
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        await vm.refreshModels(providerId: "claude")

        XCTAssertEqual(vm.modelsError(for: "claude"), .unauthorized)
        XCTAssertEqual(vm.modelsErrorMessage(for: "claude"), "Key rejected by provider")
    }

    func testRefreshModelsRateLimitedExposesRetryAfter() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        providers.modelsResult = .failure(BackendError.httpStatus(
            429,
            body: APIErrorBody(
                type: "about:blank",
                title: "rate_limited",
                status: 429,
                detail: "slow down",
                code: "provider.rate_limited"
            )
        ))
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        await vm.refreshModels(providerId: "claude")

        XCTAssertEqual(vm.modelsError(for: "claude"), .rateLimited(retryAfter: nil))
        XCTAssertTrue(
            vm.modelsErrorMessage(for: "claude").lowercased().contains("rate"),
            "rate-limited message should mention rate; got '\(vm.modelsErrorMessage(for: "claude"))'"
        )
    }

    func testRefreshModelsUnconfiguredExposesTypedError() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        providers.modelsResult = .failure(BackendError.httpStatus(
            409,
            body: APIErrorBody(
                type: "about:blank",
                title: "provider_unconfigured",
                status: 409,
                code: "provider.unconfigured"
            )
        ))
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        await vm.refreshModels(providerId: "claude")

        XCTAssertEqual(vm.modelsError(for: "claude"), .unconfigured)
        XCTAssertTrue(
            vm.modelsErrorMessage(for: "claude").lowercased().contains("api key"),
            "unconfigured message should suggest adding an API key; got '\(vm.modelsErrorMessage(for: "claude"))'"
        )
    }

    func testRefreshProvidersTransportErrorSetsRetryableBanner() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        providers.listResult = .failure(BackendError.transport("connection refused"))
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        await vm.refreshProviders()

        XCTAssertEqual(vm.providersBannerError, .transport)
        XCTAssertNotNil(vm.providersBannerError, "transport failure must surface a retryable banner")
    }

    func testRefreshProvidersClearsBannerOnSuccess() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        providers.listResult = .failure(BackendError.transport("nope"))
        let vm = SettingsViewModel(settings: gateway, providers: providers)
        await vm.refreshProviders()
        XCTAssertNotNil(vm.providersBannerError)

        // Now flip the gateway to succeed and retry.
        providers.listResult = .success([
            Provider(id: "claude", displayName: "Claude", configured: false, capabilities: ProviderCapabilities(
                streaming: true, tools: true, vision: false, systemPrompt: true, maxContextTokens: 200_000
            )),
        ])
        await vm.refreshProviders()
        XCTAssertNil(vm.providersBannerError, "successful refresh must clear the banner")
    }

    // T3.1b #2 — empty provider state reachable; "+ Add" CTA wired.
    func testEmptyProviderStateIsReachable() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        providers.listResult = .success([])
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        await vm.refreshProviders()

        XCTAssertTrue(vm.providersList.isEmpty)
        XCTAssertNil(vm.providersBannerError)
        XCTAssertTrue(vm.shouldShowAddProviderEmptyState,
                      "empty provider list with no error must trigger the empty-state CTA")
    }

    func testEmptyProviderStateNotShownWhenLoading() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        let started = AsyncSemaphore()
        let release = AsyncSemaphore()
        providers.beforeList = { @Sendable in
            await started.signal()
            await release.wait()
        }
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        let task = Task { await vm.refreshProviders() }
        await started.wait()

        XCTAssertTrue(vm.isLoadingProviders, "expected loading flag while in flight")
        XCTAssertFalse(vm.shouldShowAddProviderEmptyState,
                       "empty-state CTA must hide while a load is in progress")

        await release.signal()
        await task.value
    }

    // T3.1b #4 — loading states reachable while gateway calls in flight.
    func testIsLoadingModelsTrueWhileRefreshInFlight() async {
        let gateway = FakeSettingsGateway(initial: Settings())
        let providers = FakeProvidersGateway()
        let started = AsyncSemaphore()
        let release = AsyncSemaphore()
        providers.beforeListModels = { @Sendable _ in
            await started.signal()
            await release.wait()
        }
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        let task = Task { await vm.refreshModels(providerId: "claude") }
        await started.wait()

        XCTAssertTrue(vm.isLoadingModels.contains("claude"))

        await release.signal()
        await task.value

        XCTAssertFalse(vm.isLoadingModels.contains("claude"))
    }
}
