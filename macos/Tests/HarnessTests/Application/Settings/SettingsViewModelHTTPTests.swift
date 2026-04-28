import XCTest
@testable import HarnessApp

/// Integration tests that wire the real adapter chain
/// (`HTTPClient` + `ProvidersGatewayAdapter` + `SettingsGatewayAdapter`)
/// to a mocked HTTP backend via `MockHTTPProtocol`. These exercise the
/// SettingsViewModel through the production code paths but with a
/// scriptable backend so we can assert behaviors the real Claude
/// provider can't deterministically reproduce in CI (rejected key,
/// canned model list).
@MainActor
final class SettingsViewModelHTTPTests: XCTestCase {
    private var router: MockHTTPProtocol.Router!

    override func setUp() {
        super.setUp()
        router = MockHTTPProtocol.Router()
        MockHTTPProtocol.install(router: router)
    }

    override func tearDown() {
        MockHTTPProtocol.reset()
        router = nil
        super.tearDown()
    }

    private func makeAdapters() -> (SettingsGatewayAdapter, ProvidersGatewayAdapter) {
        let url = URL(string: "http://127.0.0.1:9999")!
        let session = MockHTTPProtocol.makeSession()
        let client = HTTPClient(baseURL: url, token: "test-token", session: session)
        return (SettingsGatewayAdapter(client: client), ProvidersGatewayAdapter(client: client))
    }

    /// T2.2 #2 — bad key surfaces a typed error.
    /// When the backend's provider endpoints return 401 (the shape we'd
    /// see if the server translated `Unauthorized` upstream), the view
    /// model exposes `providerError == .unauthorized` and the per-provider
    /// models error message is user-readable.
    func testUnauthorizedFromBackendSurfacesTypedError() async {
        router.register { request in
            let path = request.url?.path ?? ""
            switch path {
            case "/v1/providers/claude/models",
                 "/v1/providers/claude/config":
                let body = #"{"type":"about:blank","title":"unauthorized","status":401,"detail":"api key rejected"}"#
                return .init(
                    status: 401,
                    headers: ["Content-Type": "application/problem+json"],
                    body: Data(body.utf8)
                )
            default:
                return .init(status: 599, body: Data("unexpected: \(path)".utf8))
            }
        }

        let (settings, providers) = makeAdapters()
        let vm = SettingsViewModel(settings: settings, providers: providers)

        // Save flow: 401 → typed `.unauthorized` on the form's error slot.
        await vm.upsertProvider(id: "claude", apiKey: "sk-bad", baseUrl: nil)
        XCTAssertEqual(vm.providerError, .unauthorized)

        // Models flow: 401 → user-readable error in `modelsLoadError`.
        await vm.refreshModels(providerId: "claude")
        let message = vm.modelsLoadError["claude"] ?? ""
        XCTAssertFalse(message.isEmpty, "expected a user-readable models error")
        XCTAssertTrue(
            message.lowercased().contains("api key")
                || message.lowercased().contains("unauthorized")
                || message.lowercased().contains("rejected"),
            "unauthorized models error should mention api key / unauthorized / rejected — got '\(message)'"
        )
    }

    /// T2.2 #3 — model list refresh.
    /// `refreshModels("claude")` populates `vm.models["claude"]` with the
    /// `[Model]` returned by the backend.
    func testRefreshModelsExposesBackendModels() async {
        let claude = Model(id: "claude-3-5-sonnet", displayName: "Claude 3.5 Sonnet", contextWindow: 200_000)
        let haiku = Model(id: "claude-3-5-haiku", displayName: "Claude 3.5 Haiku", contextWindow: 200_000)
        router.register { request in
            guard request.url?.path == "/v1/providers/claude/models" else {
                return .init(status: 599, body: Data("unexpected".utf8))
            }
            let body = """
            {"models":[
                {"id":"claude-3-5-sonnet","display_name":"Claude 3.5 Sonnet","context_window":200000},
                {"id":"claude-3-5-haiku","display_name":"Claude 3.5 Haiku","context_window":200000}
            ]}
            """
            return .init(
                status: 200,
                headers: ["Content-Type": "application/json"],
                body: Data(body.utf8)
            )
        }

        let (settings, providers) = makeAdapters()
        let vm = SettingsViewModel(settings: settings, providers: providers)

        await vm.refreshModels(providerId: "claude")

        XCTAssertNil(vm.modelsLoadError["claude"])
        XCTAssertEqual(vm.models["claude"]?.map(\.id), [claude.id, haiku.id])
        XCTAssertEqual(vm.models["claude"]?.first?.displayName, claude.displayName)
    }
}
