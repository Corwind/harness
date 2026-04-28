import SwiftUI

public struct ChatView: View {
    @State private var viewModel: ChatViewModel
    @State private var draft: String = ""

    public init(viewModel: ChatViewModel) {
        _viewModel = State(initialValue: viewModel)
    }

    public var body: some View {
        VStack(spacing: 0) {
            messageList
            Divider()
            if let error = viewModel.error {
                errorBanner(error)
            }
            Composer(
                text: $draft,
                isStreaming: viewModel.isStreaming,
                onSend: { send() },
                onCancel: { viewModel.cancel() }
            )
        }
        .frame(minWidth: 480, minHeight: 360)
    }

    @ViewBuilder
    private var messageList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 4) {
                    ForEach(viewModel.messages) { message in
                        MessageBubble(message: message)
                            .id(message.id)
                    }
                    Color.clear.frame(height: 1).id(Self.bottomAnchorId)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
            }
            .onChange(of: viewModel.messages.count) { _, _ in
                scrollToBottom(proxy: proxy)
            }
            .onChange(of: trailingTextDigest) { _, _ in
                scrollToBottom(proxy: proxy)
            }
        }
    }

    private static let bottomAnchorId = "chat.bottom.anchor"

    private var trailingTextDigest: String {
        guard let last = viewModel.messages.last else { return "" }
        let toolPart = last.toolCalls
            .map { "\($0.id):\($0.stdout.count):\($0.stderr.count):\($0.partialJSON.count)" }
            .joined(separator: "|")
        return "\(last.id):\(last.text.count):\(toolPart)"
    }

    private func scrollToBottom(proxy: ScrollViewProxy) {
        withAnimation(.easeOut(duration: 0.15)) {
            proxy.scrollTo(Self.bottomAnchorId, anchor: .bottom)
        }
    }

    private func errorBanner(_ error: ChatError) -> some View {
        HStack(spacing: 6) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.red)
            Text(error.message)
                .font(.caption)
                .foregroundStyle(.red)
                .textSelection(.enabled)
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(Color.red.opacity(0.08))
    }

    private func send() {
        let text = draft
        draft = ""
        Task {
            await viewModel.send(text)
        }
    }
}

#if DEBUG
extension ChatViewModel {
    public static var preview: ChatViewModel {
        let gateway = PreviewRunGateway()
        let messages = PreviewMessageGateway()
        return ChatViewModel(
            runGateway: gateway,
            messageGateway: messages,
            conversationId: "preview"
        )
    }
}

private struct PreviewRunGateway: RunGateway {
    func events(runId: String, lastEventId: String?) async throws -> AsyncThrowingStream<RunEvent, Error> {
        AsyncThrowingStream { continuation in
            continuation.finish()
        }
    }
    func cancel(runId: String) async throws {}
}

private struct PreviewMessageGateway: MessageGateway {
    func list(conversationId: String, limit: Int?, afterOrdinal: Int?) async throws -> [Message] { [] }
    func post(conversationId: String, _ request: PostMessageRequest) async throws -> RunHandle {
        RunHandle(runId: "preview-run", conversationId: conversationId, messageId: "preview-msg")
    }
}

#Preview {
    ChatView(viewModel: .preview)
}
#endif
