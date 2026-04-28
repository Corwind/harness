import SwiftUI

/// Settings → Sandboxes tab. Lists all templates (built-ins + user),
/// supports create/edit/delete, and surfaces validate results inline.
struct SandboxesTab: View {
    @Environment(\.theme) private var theme
    @Bindable var viewModel: SandboxTemplatesViewModel

    @State private var selectedId: String?
    @State private var isCreating: Bool = false

    var body: some View {
        HSplitView {
            sidebar
                .frame(minWidth: 220)
            detail
                .frame(minWidth: 420)
        }
        .background(theme.background)
        .foregroundStyle(theme.text)
        .task {
            if viewModel.templates.isEmpty {
                await viewModel.load()
            }
        }
    }

    private var sidebar: some View {
        VStack(spacing: 0) {
            List(selection: $selectedId) {
                Section("Built-in") {
                    ForEach(viewModel.templates.filter(\.isBuiltin), id: \.id) { row($0) }
                }
                Section("Custom") {
                    ForEach(viewModel.templates.filter { !$0.isBuiltin }, id: \.id) { row($0) }
                }
            }
            .listStyle(.sidebar)

            HStack {
                Button {
                    isCreating = true
                    selectedId = nil
                } label: {
                    Label("New", systemImage: "plus")
                }
                Spacer()
                if let id = selectedId,
                   let target = viewModel.templates.first(where: { $0.id == id }),
                   viewModel.canDelete(target) {
                    Button(role: .destructive) {
                        Task {
                            await viewModel.deleteTemplate(id: id)
                            selectedId = nil
                        }
                    } label: {
                        Label("Delete", systemImage: "trash")
                    }
                }
            }
            .padding(8)
        }
    }

    private func row(_ template: SandboxTemplate) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(template.name)
                if let desc = template.description, !desc.isEmpty {
                    Text(desc)
                        .font(.caption)
                        .foregroundStyle(theme.mutedText)
                        .lineLimit(1)
                }
            }
            Spacer()
            if let result = viewModel.validationResults[template.id] {
                Image(systemName: result.valid ? "checkmark.circle.fill" : "xmark.octagon.fill")
                    .foregroundStyle(result.valid ? theme.success : theme.error)
                    .accessibilityHidden(true)
            }
        }
        .tag(Optional(template.id))
    }

    @ViewBuilder
    private var detail: some View {
        if isCreating {
            SandboxTemplateEditor(
                viewModel: viewModel,
                editing: nil,
                onSaved: { saved in
                    isCreating = false
                    selectedId = saved.id
                },
                onCancel: { isCreating = false }
            )
        } else if let id = selectedId,
                  let template = viewModel.templates.first(where: { $0.id == id }) {
            if template.isBuiltin {
                builtinPreview(template)
            } else {
                SandboxTemplateEditor(
                    viewModel: viewModel,
                    editing: template,
                    onSaved: { _ in },
                    onCancel: { selectedId = nil }
                )
                .id(template.id)
            }
        } else {
            VStack(spacing: 8) {
                Spacer()
                Text("Select a sandbox template to view or edit, or click New to add one.")
                    .foregroundStyle(theme.mutedText)
                    .multilineTextAlignment(.center)
                Spacer()
            }
            .frame(maxWidth: .infinity)
        }
    }

    private func builtinPreview(_ template: SandboxTemplate) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(template.name).font(.title3).foregroundStyle(theme.text)
                Text("built-in").font(.caption)
                    .padding(.horizontal, 6).padding(.vertical, 2)
                    .background(theme.surface)
                    .foregroundStyle(theme.mutedText)
                    .clipShape(RoundedRectangle(cornerRadius: 4))
            }
            if let desc = template.description, !desc.isEmpty {
                Text(desc).foregroundStyle(theme.mutedText)
            }
            Text("Profile (read-only)")
                .font(.caption).foregroundStyle(theme.mutedText)
            ScrollView {
                Text(template.profile)
                    .font(.system(.body, design: .monospaced))
                    .foregroundStyle(theme.codeText)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
                    .background(theme.codeBackground)
                    .textSelection(.enabled)
            }
            HStack {
                Button(viewModel.isValidating.contains(template.id) ? "Validating…" : "Validate") {
                    Task { await viewModel.validate(id: template.id) }
                }
                .disabled(viewModel.isValidating.contains(template.id))
                Spacer()
            }
            if let result = viewModel.validationResults[template.id] {
                if result.valid {
                    Label("Profile validates", systemImage: "checkmark.circle.fill")
                        .foregroundStyle(theme.success)
                } else {
                    VStack(alignment: .leading, spacing: 4) {
                        Label("Profile failed validation", systemImage: "xmark.octagon.fill")
                            .foregroundStyle(theme.error)
                        if let stderr = result.stderr {
                            Text(stderr)
                                .font(.system(.caption, design: .monospaced))
                                .foregroundStyle(theme.codeText)
                                .padding(8)
                                .background(theme.codeBackground)
                                .textSelection(.enabled)
                        }
                    }
                }
            }
            Spacer()
        }
        .padding()
    }
}
