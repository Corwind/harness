import SwiftUI

struct MessageBubble: View {
    let message: ChatMessage

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            if message.role == .user {
                Spacer(minLength: 40)
                bubble
                    .frame(maxWidth: 520, alignment: .trailing)
            } else {
                bubble
                    .frame(maxWidth: 520, alignment: .leading)
                Spacer(minLength: 40)
            }
        }
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private var bubble: some View {
        VStack(alignment: alignment, spacing: 6) {
            if !message.text.isEmpty {
                Text(message.text)
                    .textSelection(.enabled)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(bubbleBackground)
                    .foregroundStyle(bubbleForeground)
                    .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
            }

            ForEach(message.toolCalls) { call in
                ToolCallDisclosure(call: call)
            }

            if message.status == .errored {
                Label("error", systemImage: "exclamationmark.triangle.fill")
                    .font(.caption)
                    .foregroundStyle(.red)
            } else if message.status == .cancelled {
                Label("cancelled", systemImage: "stop.circle")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var alignment: HorizontalAlignment {
        message.role == .user ? .trailing : .leading
    }

    private var bubbleBackground: Color {
        switch message.role {
        case .user: return Color.accentColor.opacity(0.85)
        case .assistant: return Color.gray.opacity(0.15)
        case .tool, .system: return Color.gray.opacity(0.08)
        }
    }

    private var bubbleForeground: Color {
        message.role == .user ? .white : .primary
    }
}

struct ToolCallDisclosure: View {
    let call: ChatToolCall
    @State private var expanded: Bool = false

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: 6) {
                if let input = call.input {
                    Text("input").font(.caption).foregroundStyle(.secondary)
                    Text(formatJSON(input))
                        .font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                } else if !call.partialJSON.isEmpty {
                    Text("input (streaming)").font(.caption).foregroundStyle(.secondary)
                    Text(call.partialJSON)
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(.secondary)
                }

                if !call.stdout.isEmpty {
                    Text("stdout").font(.caption).foregroundStyle(.secondary)
                    Text(call.stdout)
                        .font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                }

                if !call.stderr.isEmpty {
                    Text("stderr").font(.caption).foregroundStyle(.red)
                    Text(call.stderr)
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(.red)
                        .textSelection(.enabled)
                }

                if let exitCode = call.exitCode {
                    Text("exit \(exitCode) · \(call.durationMs ?? 0) ms")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }

                if let errMsg = call.errorMessage {
                    Text("\(call.errorCode ?? "error"): \(errMsg)")
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            .padding(.vertical, 4)
        } label: {
            HStack(spacing: 6) {
                statusIcon
                Text(call.name).font(.callout).fontWeight(.medium)
                if let kind = call.kind {
                    Text(kind == .external ? "external" : "in-process")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                Spacer()
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color.gray.opacity(0.08))
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
    }

    @ViewBuilder
    private var statusIcon: some View {
        switch call.status {
        case .streamingInput:
            Image(systemName: "ellipsis.circle").foregroundStyle(.secondary)
        case .executing:
            ProgressView().controlSize(.small)
        case .finished:
            Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
        case .errored:
            Image(systemName: "xmark.octagon.fill").foregroundStyle(.red)
        }
    }

    private func formatJSON(_ input: [String: JSONValue]) -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(input),
              let s = String(data: data, encoding: .utf8) else {
            return "{}"
        }
        return s
    }
}
