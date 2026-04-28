import XCTest
@testable import HarnessApp

@MainActor
final class SandboxTemplatesViewModelTests: XCTestCase {
    // T2.5 #1 — list populates from gateway; built-ins show as immutable.
    func testLoadPopulatesTemplatesFromGateway() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [
            .fixture(id: "tpl_strict", name: "strict-readonly", isBuiltin: true),
            .fixture(id: "tpl_custom", name: "my-custom", isBuiltin: false),
        ])
        let vm = SandboxTemplatesViewModel(gateway: gateway)

        await vm.load()

        XCTAssertEqual(gateway.listCallCount, 1)
        XCTAssertEqual(vm.templates.map(\.id), ["tpl_strict", "tpl_custom"])
        XCTAssertNil(vm.loadError)
    }

    func testBuiltinsAreReadOnly() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [
            .fixture(id: "tpl_strict", name: "strict-readonly", isBuiltin: true),
            .fixture(id: "tpl_custom", name: "my-custom", isBuiltin: false),
        ])
        let vm = SandboxTemplatesViewModel(gateway: gateway)
        await vm.load()

        let builtin = vm.templates.first(where: { $0.id == "tpl_strict" })!
        let custom = vm.templates.first(where: { $0.id == "tpl_custom" })!
        XCTAssertFalse(vm.canEdit(builtin))
        XCTAssertFalse(vm.canDelete(builtin))
        XCTAssertTrue(vm.canEdit(custom))
        XCTAssertTrue(vm.canDelete(custom))
    }

    // T2.5 #2 — create flow.
    func testCreateAddsTemplateAndCallsGatewayOnce() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [])
        let vm = SandboxTemplatesViewModel(gateway: gateway)
        await vm.load()

        await vm.createTemplate(
            name: "writeable-tmp",
            description: "scratch",
            profile: "(version 1) (allow default)"
        )

        XCTAssertEqual(gateway.createCallCount, 1)
        XCTAssertEqual(gateway.lastCreate?.name, "writeable-tmp")
        XCTAssertEqual(gateway.lastCreate?.profile, "(version 1) (allow default)")
        XCTAssertEqual(vm.templates.count, 1)
        XCTAssertEqual(vm.templates.first?.name, "writeable-tmp")
        XCTAssertNil(vm.createError)
    }

    func testCreateRejectsEmptyNameAndDoesNotHitGateway() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [])
        let vm = SandboxTemplatesViewModel(gateway: gateway)
        await vm.load()

        await vm.createTemplate(name: "  ", description: nil, profile: "(version 1)")
        XCTAssertEqual(gateway.createCallCount, 0)
        XCTAssertEqual(vm.createError, .emptyName)
    }

    func testCreateRejectsEmptyProfile() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [])
        let vm = SandboxTemplatesViewModel(gateway: gateway)
        await vm.load()

        await vm.createTemplate(name: "ok", description: nil, profile: "  ")
        XCTAssertEqual(gateway.createCallCount, 0)
        XCTAssertEqual(vm.createError, .emptyProfile)
    }

    // T2.5 #3 — validate.
    func testValidateMalformedProfileRendersStderrInline() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [
            .fixture(id: "tpl_bad", name: "bad", isBuiltin: false),
        ])
        gateway.validateResult = .success(
            .init(valid: false, stderr: "syntax error at line 1: unexpected token")
        )
        let vm = SandboxTemplatesViewModel(gateway: gateway)
        await vm.load()

        await vm.validate(id: "tpl_bad")

        XCTAssertEqual(gateway.validateCallCount, 1)
        XCTAssertEqual(gateway.lastValidatedId, "tpl_bad")
        let result = vm.validationResults["tpl_bad"]
        XCTAssertNotNil(result)
        XCTAssertEqual(result?.valid, false)
        XCTAssertEqual(result?.stderr, "syntax error at line 1: unexpected token")
    }

    func testValidateValidProfileShowsSuccessNoStderr() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [
            .fixture(id: "tpl_ok", name: "ok"),
        ])
        gateway.validateResult = .success(.init(valid: true, stderr: nil))
        let vm = SandboxTemplatesViewModel(gateway: gateway)
        await vm.load()

        await vm.validate(id: "tpl_ok")

        XCTAssertEqual(vm.validationResults["tpl_ok"]?.valid, true)
        XCTAssertNil(vm.validationResults["tpl_ok"]?.stderr)
    }

    // T2.5 #4 — delete custom; built-in is impossible from the UI.
    func testDeleteCustomRemovesIt() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [
            .fixture(id: "tpl_custom", name: "my-custom"),
        ])
        let vm = SandboxTemplatesViewModel(gateway: gateway)
        await vm.load()

        await vm.deleteTemplate(id: "tpl_custom")

        XCTAssertEqual(gateway.deleteCallCount, 1)
        XCTAssertEqual(gateway.deletedIds, ["tpl_custom"])
        XCTAssertTrue(vm.templates.isEmpty)
    }

    func testDeleteBuiltinIsRejectedAtViewModelLayer() async {
        let gateway = FakeSandboxTemplatesGateway(seeded: [
            .fixture(id: "tpl_strict", name: "strict-readonly", isBuiltin: true),
        ])
        let vm = SandboxTemplatesViewModel(gateway: gateway)
        await vm.load()

        await vm.deleteTemplate(id: "tpl_strict")

        XCTAssertEqual(gateway.deleteCallCount, 0, "built-in delete must never reach the gateway")
        XCTAssertEqual(vm.templates.count, 1)
    }
}
