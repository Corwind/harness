import SwiftUI

public struct Theme: Equatable, Sendable {
    public let background: Color
    public let surface: Color
    public let accent: Color
    public let text: Color
    public let mutedText: Color
    public let codeBackground: Color
    public let codeText: Color
    public let separator: Color
    public let error: Color
    public let success: Color

    public init(
        background: Color,
        surface: Color,
        accent: Color,
        text: Color,
        mutedText: Color,
        codeBackground: Color,
        codeText: Color,
        separator: Color,
        error: Color,
        success: Color
    ) {
        self.background = background
        self.surface = surface
        self.accent = accent
        self.text = text
        self.mutedText = mutedText
        self.codeBackground = codeBackground
        self.codeText = codeText
        self.separator = separator
        self.error = error
        self.success = success
    }

    public static let light: Theme = Theme(
        background: Color(white: 1.00),
        surface: Color(white: 0.96),
        accent: Color(red: 0.20, green: 0.40, blue: 0.92),
        text: Color(white: 0.10),
        mutedText: Color(white: 0.42),
        codeBackground: Color(white: 0.94),
        codeText: Color(white: 0.10),
        separator: Color(white: 0.85),
        error: Color(red: 0.78, green: 0.16, blue: 0.16),
        success: Color(red: 0.16, green: 0.55, blue: 0.30)
    )

    public static let dark: Theme = Theme(
        background: Color(white: 0.10),
        surface: Color(white: 0.16),
        accent: Color(red: 0.42, green: 0.62, blue: 1.00),
        text: Color(white: 0.96),
        mutedText: Color(white: 0.66),
        codeBackground: Color(white: 0.18),
        codeText: Color(white: 0.96),
        separator: Color(white: 0.28),
        error: Color(red: 0.94, green: 0.40, blue: 0.40),
        success: Color(red: 0.46, green: 0.80, blue: 0.55)
    )
}
