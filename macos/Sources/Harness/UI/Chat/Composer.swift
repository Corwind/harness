import SwiftUI

struct Composer: View {
    @Binding var text: String
    let isStreaming: Bool
    let onSend: () -> Void
    let onCancel: () -> Void

    var body: some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextField(
                "Message",
                text: $text,
                axis: .vertical
            )
            .textFieldStyle(.roundedBorder)
            .lineLimit(1...8)
            .disabled(isStreaming)
            .onSubmit(submitIfPossible)

            if isStreaming {
                Button(role: .cancel, action: onCancel) {
                    Image(systemName: "stop.circle.fill")
                        .imageScale(.large)
                }
                .keyboardShortcut(".", modifiers: .command)
                .help("Cancel (⌘.)")
            } else {
                Button(action: submitIfPossible) {
                    Image(systemName: "arrow.up.circle.fill")
                        .imageScale(.large)
                }
                .disabled(trimmed.isEmpty)
                .keyboardShortcut(.return, modifiers: .command)
                .help("Send (⌘↩)")
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    private var trimmed: String {
        text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func submitIfPossible() {
        guard !trimmed.isEmpty, !isStreaming else { return }
        onSend()
    }
}
