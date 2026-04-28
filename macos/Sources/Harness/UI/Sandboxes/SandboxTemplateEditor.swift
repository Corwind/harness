import SwiftUI

/// Form for creating / editing a sandbox template. Used by the Sandboxes
/// tab. Validation happens via `SandboxTemplatesViewModel.validate(id:)`
/// after the template has been saved at least once; the inline result
/// surfaces stderr from a failed dry-run.
struct SandboxTemplateEditor: View {
    @Environment(\.theme) private var theme
    @Bindable var viewModel: SandboxTemplatesViewModel

    let editing: SandboxTemplate?
    let onSaved: (SandboxTemplate) -> Void
    let onCancel: () -> Void

    @State private var name: String = ""
    @State private var description: String = ""
    @State private var profile: String = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(editing == nil ? "New sandbox template" : "Edit sandbox template")
                .font(.title3)
                .foregroundStyle(theme.text)

            Form {
                TextField("Name", text: $name)
                    .textFieldStyle(.roundedBorder)
                TextField("Description (optional)", text: $description)
                    .textFieldStyle(.roundedBorder)
            }

            Text("Profile (SBPL)")
                .font(.caption)
                .foregroundStyle(theme.mutedText)

            TextEditor(text: $profile)
                .font(.system(.body, design: .monospaced))
                .frame(minHeight: 200)
                .background(theme.codeBackground)
                .foregroundStyle(theme.codeText)
                .border(theme.separator)

            if let editing, let result = viewModel.validationResults[editing.id] {
                validationSummary(for: editing, result: result)
            }

            if let createError = viewModel.createError {
                Text(createError.userMessage)
                    .font(.caption)
                    .foregroundStyle(theme.error)
            }

            HStack {
                if let editing {
                    Button(viewModel.isValidating.contains(editing.id) ? "Validating…" : "Validate") {
                        Task { await viewModel.validate(id: editing.id) }
                    }
                    .disabled(viewModel.isValidating.contains(editing.id))
                }
                Spacer()
                Button("Cancel", action: onCancel)
                Button("Save") {
                    Task { await save() }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(viewModel.isCreating || (editing?.isBuiltin ?? false))
            }
        }
        .padding()
        .background(theme.background)
        .onAppear { hydrate() }
    }

    @ViewBuilder
    private func validationSummary(for template: SandboxTemplate, result: ValidateSandboxResult) -> some View {
        if result.valid {
            HStack(spacing: 6) {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(theme.success)
                Text("Profile validates")
                    .font(.callout)
                    .foregroundStyle(theme.success)
            }
        } else {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Image(systemName: "xmark.octagon.fill")
                        .foregroundStyle(theme.error)
                    Text("Profile failed validation")
                        .font(.callout)
                        .foregroundStyle(theme.error)
                }
                if let stderr = result.stderr, !stderr.isEmpty {
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

    private func hydrate() {
        if let editing {
            name = editing.name
            description = editing.description ?? ""
            profile = editing.profile
        }
    }

    private func save() async {
        if let editing {
            await viewModel.updateTemplate(
                id: editing.id,
                name: name,
                description: description.isEmpty ? nil : description,
                profile: profile
            )
            if let updated = viewModel.templates.first(where: { $0.id == editing.id }) {
                onSaved(updated)
            }
        } else {
            await viewModel.createTemplate(
                name: name,
                description: description.isEmpty ? nil : description,
                profile: profile
            )
            if viewModel.createError == nil, let created = viewModel.templates.last {
                onSaved(created)
            }
        }
    }
}
