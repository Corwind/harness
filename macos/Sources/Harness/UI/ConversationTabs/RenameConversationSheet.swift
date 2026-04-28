import SwiftUI

struct RenameConversationSheet: View {
    let conversation: Conversation
    let onRename: (String) -> Void
    let onCancel: () -> Void

    @State private var title: String

    init(
        conversation: Conversation,
        onRename: @escaping (String) -> Void,
        onCancel: @escaping () -> Void
    ) {
        self.conversation = conversation
        self.onRename = onRename
        self.onCancel = onCancel
        _title = State(initialValue: conversation.title)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Rename conversation")
                .font(.headline)
            TextField("Title", text: $title)
                .textFieldStyle(.roundedBorder)
                .onSubmit(submit)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel, action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button("Rename", action: submit)
                    .keyboardShortcut(.defaultAction)
                    .disabled(trimmed.isEmpty)
            }
        }
        .padding(16)
        .frame(minWidth: 360)
    }

    private var trimmed: String {
        title.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func submit() {
        guard !trimmed.isEmpty else { return }
        onRename(trimmed)
    }
}
