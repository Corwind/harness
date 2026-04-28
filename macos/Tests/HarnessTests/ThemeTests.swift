import XCTest
@testable import HarnessApp

@MainActor
final class ThemeTests: XCTestCase {
    // T1.J behavior 1 — semantic-token coverage:
    // The Settings + Sandboxes views (and chat MessageBubble when it lands)
    // must read every color from the theme — no `Color.red`, `Color.blue`,
    // etc. literals. We grep our own sources to enforce this.
    func testSettingsViewsUseOnlyThemeColors() throws {
        let root = Self.repoRoot()
        let dirs = [
            root.appendingPathComponent("Sources/Harness/UI/Settings", isDirectory: true),
            root.appendingPathComponent("Sources/Harness/UI/Sandboxes", isDirectory: true),
        ]
        var sawAtLeastOne = false
        for dir in dirs {
            let urls = try filesIn(dir, withSuffix: ".swift")
            for url in urls {
                sawAtLeastOne = true
                let source = try String(contentsOf: url, encoding: .utf8)
                for forbidden in Self.forbiddenColorLiterals {
                    XCTAssertFalse(
                        source.contains(forbidden),
                        "\(url.lastPathComponent) uses forbidden color literal '\(forbidden)' — read from the Theme instead"
                    )
                }
            }
        }
        XCTAssertTrue(sawAtLeastOne, "Expected at least one file under UI/Settings/ or UI/Sandboxes/")
    }

    // T1.J behavior 2 — live theme switch:
    // Switching `appTheme` from `.light` to `.dark` flips `currentTheme` to the
    // matching value (and back). This is what every view observes via @Environment.
    func testLiveThemeSwitchUpdatesCurrentTheme() async {
        let gateway = FakeSettingsGateway(initial: Settings(theme: .light))
        let providers = FakeProvidersGateway()
        let vm = SettingsViewModel(settings: gateway, providers: providers)

        await vm.load()
        XCTAssertEqual(vm.appTheme, .light)
        XCTAssertEqual(vm.currentTheme.background, Theme.light.background)

        await vm.setTheme(.dark)
        XCTAssertEqual(vm.appTheme, .dark)
        XCTAssertEqual(vm.currentTheme.background, Theme.dark.background)

        await vm.setTheme(.light)
        XCTAssertEqual(vm.currentTheme.background, Theme.light.background)
    }

    // MARK: - helpers

    private static let forbiddenColorLiterals: [String] = [
        "Color.red",
        "Color.blue",
        "Color.green",
        "Color.yellow",
        "Color.orange",
        "Color.pink",
        "Color.purple",
        "Color.gray",
        "Color.black",
        "Color.white",
        "Color(red:",
        "Color(hue:",
        "Color(.sRGB",
    ]

    private func filesIn(_ root: URL, withSuffix suffix: String) throws -> [URL] {
        let fm = FileManager.default
        guard let it = fm.enumerator(at: root, includingPropertiesForKeys: nil) else {
            return []
        }
        var out: [URL] = []
        for case let url as URL in it where url.lastPathComponent.hasSuffix(suffix) {
            out.append(url)
        }
        return out
    }

    private static func repoRoot() -> URL {
        // This file lives at .../macos/Tests/HarnessTests/ThemeTests.swift.
        // The package root is two directories up.
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent() // HarnessTests/
            .deletingLastPathComponent() // Tests/
            .deletingLastPathComponent() // macos/ (package root)
    }
}
