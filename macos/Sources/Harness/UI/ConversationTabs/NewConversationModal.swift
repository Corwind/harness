import SwiftUI

struct NewConversationModal: View {
    let providers: [Provider]
    let models: [String: [Model]]
    let sandboxTemplates: [SandboxTemplate]
    let onCreate: (_ providerId: String, _ model: String, _ title: String?, _ sandboxTemplateId: String?) -> Void
    let onCancel: () -> Void

    @State private var providerId: String = ""
    @State private var model: String = ""
    @State private var title: String = ""
    @State private var sandboxTemplateId: String? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("New conversation")
                .font(.headline)

            Form {
                Picker("Provider", selection: $providerId) {
                    ForEach(providers, id: \.id) { provider in
                        Text(provider.displayName).tag(provider.id)
                    }
                }

                Picker("Model", selection: $model) {
                    ForEach(modelsForActiveProvider, id: \.id) { m in
                        Text(m.displayName).tag(m.id)
                    }
                }
                .disabled(modelsForActiveProvider.isEmpty)

                Picker("Sandbox", selection: sandboxBinding) {
                    Text("None — tools blocked").tag(String?.none)
                    ForEach(sandboxTemplates, id: \.id) { template in
                        Text(template.name).tag(String?.some(template.id))
                    }
                }

                TextField("Title (optional)", text: $title)
            }

            if sandboxTemplateId == nil {
                Label(
                    "No sandbox attached — external tools will refuse to run.",
                    systemImage: "exclamationmark.triangle.fill"
                )
                .font(.caption)
                .foregroundStyle(.orange)
            }

            HStack {
                Spacer()
                Button("Cancel", role: .cancel, action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button("Create") {
                    let trimmedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
                    onCreate(
                        providerId,
                        model,
                        trimmedTitle.isEmpty ? nil : trimmedTitle,
                        sandboxTemplateId
                    )
                }
                .keyboardShortcut(.defaultAction)
                .disabled(providerId.isEmpty || model.isEmpty)
            }
        }
        .padding(20)
        .frame(minWidth: 420)
        .onAppear {
            if providerId.isEmpty, let firstProvider = providers.first {
                providerId = firstProvider.id
            }
            updateModelDefault()
        }
        .onChange(of: providerId) { _, _ in
            updateModelDefault()
        }
    }

    private var modelsForActiveProvider: [Model] {
        models[providerId] ?? []
    }

    private var sandboxBinding: Binding<String?> {
        Binding(
            get: { sandboxTemplateId },
            set: { sandboxTemplateId = $0 }
        )
    }

    private func updateModelDefault() {
        if model.isEmpty || modelsForActiveProvider.first(where: { $0.id == model }) == nil {
            model = modelsForActiveProvider.first?.id ?? ""
        }
    }
}
