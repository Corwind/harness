import SwiftUI

private struct ThemeEnvironmentKey: EnvironmentKey {
    static let defaultValue: Theme = .light
}

public extension EnvironmentValues {
    var theme: Theme {
        get { self[ThemeEnvironmentKey.self] }
        set { self[ThemeEnvironmentKey.self] = newValue }
    }
}

public extension View {
    /// Inject a `Theme` into the SwiftUI environment for this subtree.
    func theme(_ theme: Theme) -> some View {
        environment(\.theme, theme)
    }
}
