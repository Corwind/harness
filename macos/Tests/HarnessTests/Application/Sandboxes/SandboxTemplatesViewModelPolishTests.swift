import XCTest
@testable import HarnessApp

@MainActor
final class SandboxTemplatesViewModelPolishTests: XCTestCase {
    // T3.1b #3 — empty custom-templates state shows only built-ins.
    func testEmptyCustomTemplatesStateExposed() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [
            .fixture(id: "tpl_strict", name: "strict-readonly", isBuiltin: true),
            .fixture(id: "tpl_net", name: "no-network", isBuiltin: true),
            .fixture(id: "tpl_nonet", name: "network-only", isBuiltin: true),
            .fixture(id: "tpl_perm", name: "permissive-dev", isBuiltin: true),
        ])
        let vm = SandboxTemplatesViewModel(gateway: gateway)
        await vm.load()

        XCTAssertEqual(vm.customTemplateCount, 0)
        XCTAssertTrue(vm.shouldShowCustomEmptyState,
                      "empty custom-templates state must be reachable when only built-ins are seeded")
        XCTAssertEqual(vm.builtinTemplates.count, 4)
        XCTAssertTrue(vm.customTemplates.isEmpty)
    }

    func testCustomEmptyStateClearsAfterCreate() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [
            .fixture(id: "tpl_strict", name: "strict-readonly", isBuiltin: true),
        ])
        let vm = SandboxTemplatesViewModel(gateway: gateway)
        await vm.load()
        XCTAssertTrue(vm.shouldShowCustomEmptyState)

        await vm.createTemplate(name: "ok", description: nil, profile: "(version 1)")
        XCTAssertEqual(vm.customTemplateCount, 1)
        XCTAssertFalse(vm.shouldShowCustomEmptyState)
    }

    // T3.1b #1 — typed errors on load (transport surface).
    func testLoadTransportErrorSetsRetryableBanner() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [])
        gateway.listResult = .failure(BackendError.transport("connection refused"))
        let vm = SandboxTemplatesViewModel(gateway: gateway)

        await vm.load()

        XCTAssertEqual(vm.bannerError, .transport)
        XCTAssertNotNil(vm.bannerError)
    }

    func testLoadTransportBannerClearsOnSuccessfulRetry() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [])
        gateway.listResult = .failure(BackendError.transport("nope"))
        let vm = SandboxTemplatesViewModel(gateway: gateway)
        await vm.load()
        XCTAssertNotNil(vm.bannerError)

        gateway.listResult = .success([
            .fixture(id: "tpl_strict", name: "strict-readonly", isBuiltin: true),
        ])
        await vm.load()
        XCTAssertNil(vm.bannerError)
        XCTAssertEqual(vm.templates.count, 1)
    }

    // T3.1b #4 — loading reachable while gateway call is in flight.
    func testIsLoadingTrueWhileGatewayCallInFlight() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [])
        let started = AsyncSemaphore()
        let release = AsyncSemaphore()
        gateway.beforeList = { @Sendable in
            await started.signal()
            await release.wait()
        }
        let vm = SandboxTemplatesViewModel(gateway: gateway)

        let task = Task { await vm.load() }
        await started.wait()

        XCTAssertTrue(vm.isLoading, "isLoading must be true while gateway call is in flight")
        XCTAssertFalse(vm.shouldShowCustomEmptyState,
                       "empty-state CTA must hide while a load is in progress")

        await release.signal()
        await task.value

        XCTAssertFalse(vm.isLoading)
    }
}
