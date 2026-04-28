import SwiftUI

/// Reusable picker for selecting (or clearing) the sandbox template
/// attached to a conversation. Bound to a `SandboxAttachmentViewModel`
/// — used by the chat conversation header (compact) and the
/// new-conversation modal (T2.3 will integrate both).
public struct SandboxPickerView: View {
    @Environment(\.theme) private var theme
    @Bindable private var viewModel: SandboxAttachmentViewModel
    private let style: Style

    public enum Style: Sendable {
        /// Inline picker with a warning badge below.
        case standard
        /// Compact one-line label suitable for the chat header.
        case compact
    }

    public init(viewModel: SandboxAttachmentViewModel, style: Style = .standard) {
        self.viewModel = viewModel
        self.style = style
    }

    public var body: some View {
        switch style {
        case .standard: standardLayout
        case .compact: compactLayout
        }
    }

    @ViewBuilder
    private var standardLayout: some View {
        VStack(alignment: .leading, spacing: 6) {
            picker
            if viewModel.shouldShowNoSandboxWarning {
                warningBadge
            }
            if let attachError = viewModel.attachError {
                Text(attachError)
                    .font(.caption)
                    .foregroundStyle(theme.error)
            }
        }
    }

    @ViewBuilder
    private var compactLayout: some View {
        HStack(spacing: 8) {
            picker
            if viewModel.shouldShowNoSandboxWarning {
                warningBadge
            }
        }
    }

    private var picker: some View {
        Menu {
            Button("No sandbox (tools blocked)") {
                Task { await viewModel.clearActiveTemplate() }
            }
            .disabled(viewModel.isUpdating)

            if !viewModel.availableTemplates.isEmpty {
                Divider()
            }

            ForEach(viewModel.availableTemplates, id: \.id) { template in
                Button {
                    Task { await viewModel.setActiveTemplate(id: template.id) }
                } label: {
                    HStack {
                        Text(template.name)
                        if template.isBuiltin {
                            Text("built-in").font(.caption).foregroundStyle(theme.mutedText)
                        }
                    }
                }
                .disabled(viewModel.isUpdating)
            }
        } label: {
            HStack(spacing: 4) {
                Image(systemName: "shield.lefthalf.filled")
                Text(activeLabel)
                    .lineLimit(1)
                if viewModel.isUpdating {
                    ProgressView().controlSize(.small)
                }
            }
            .foregroundStyle(theme.text)
        }
        .menuStyle(.borderlessButton)
        .accessibilityLabel("Sandbox template")
    }

    private var activeLabel: String {
        if let active = viewModel.activeTemplate {
            return active.name
        }
        return "No sandbox"
    }

    private var warningBadge: some View {
        HStack(spacing: 4) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(theme.error)
            Text(viewModel.warningMessage)
                .font(.caption)
                .foregroundStyle(theme.error)
        }
        .accessibilityLabel(viewModel.warningMessage)
    }
}
