import SwiftUI

/// Optional callbacks the chat view can fire when an error banner offers
/// the user an action (Retry, Open Settings, Choose sandbox). The host
/// (RootView, ConversationTabsView) wires these to navigation / sheet
/// presentation. Default no-ops keep the view self-contained for previews.
public struct ChatViewActions {
    public var onOpenProvidersSettings: (() -> Void)?
    public var onOpenSandboxesSettings: ((_ templateId: String?) -> Void)?
    public var onChooseSandbox: (() -> Void)?

    public init(
        onOpenProvidersSettings: (() -> Void)? = nil,
        onOpenSandboxesSettings: ((_ templateId: String?) -> Void)? = nil,
        onChooseSandbox: (() -> Void)? = nil
    ) {
        self.onOpenProvidersSettings = onOpenProvidersSettings
        self.onOpenSandboxesSettings = onOpenSandboxesSettings
        self.onChooseSandbox = onChooseSandbox
    }
}

public struct ChatView: View {
    @State private var viewModel: ChatViewModel
    @State private var draft: String = ""
    private let actions: ChatViewActions
    private let activeSandboxName: String?

    public init(
        viewModel: ChatViewModel,
        activeSandboxName: String? = nil,
        actions: ChatViewActions = ChatViewActions()
    ) {
        _viewModel = State(initialValue: viewModel)
        self.activeSandboxName = activeSandboxName
        self.actions = actions
    }

    public var body: some View {
        VStack(spacing: 0) {
            content
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
    private var content: some View {
        if viewModel.isLoadingHistory && viewModel.messages.isEmpty {
            loadingState
        } else if viewModel.isEmpty {
            emptyState
        } else {
            messageList
        }
    }

    @ViewBuilder
    private var loadingState: some View {
        VStack(spacing: 12) {
            ProgressView()
            Text("Loading conversation…")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ViewBuilder
    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "bubble.left.and.bubble.right")
                .font(.largeTitle)
                .foregroundStyle(.secondary)
            Text("Start the conversation")
                .font(.headline)
            sandboxSummary
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(24)
    }

    @ViewBuilder
    private var sandboxSummary: some View {
        if let name = activeSandboxName {
            Label("Sandbox: \(name)", systemImage: "lock.shield.fill")
                .font(.caption)
                .foregroundStyle(.secondary)
        } else {
            Label(
                "No sandbox attached — external tools will refuse to run.",
                systemImage: "exclamationmark.triangle.fill"
            )
            .font(.caption)
            .foregroundStyle(.orange)
        }
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

    @ViewBuilder
    private func errorBanner(_ error: ChatError) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: errorIcon(for: error))
                .foregroundStyle(.red)
            VStack(alignment: .leading, spacing: 4) {
                Text(error.message)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .textSelection(.enabled)
                errorActionButtons(error)
            }
            Spacer()
            Button {
                viewModel.clearError()
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
            .help("Dismiss")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(Color.red.opacity(0.08))
    }

    private func errorIcon(for error: ChatError) -> String {
        switch error.kind {
        case .transport: return "wifi.exclamationmark"
        case .sessionExpired, .forbidden: return "lock.slash"
        case .providerUnauthorized, .providerUnconfigured: return "key"
        case .providerRateLimited: return "hourglass"
        case .providerUnavailable: return "exclamationmark.triangle.fill"
        case .sandboxRequired, .sandboxInvalidProfile: return "lock.shield"
        case .toolTimeout: return "clock.badge.exclamationmark"
        case .toolSpawnFailed: return "xmark.octagon.fill"
        case .server, .other: return "exclamationmark.triangle.fill"
        }
    }

    @ViewBuilder
    private func errorActionButtons(_ error: ChatError) -> some View {
        HStack(spacing: 8) {
            if error.actions.canRetry {
                Button("Retry") {
                    Task { await viewModel.retry() }
                }
                .controlSize(.small)
            }
            if error.actions.canOpenSettings {
                switch error.actions.settingsTab {
                case .providers:
                    Button("Open Settings") {
                        actions.onOpenProvidersSettings?()
                    }
                    .controlSize(.small)
                case .sandboxes(let templateId):
                    Button("Open Sandboxes") {
                        actions.onOpenSandboxesSettings?(templateId)
                    }
                    .controlSize(.small)
                case .none:
                    EmptyView()
                }
            }
            if error.actions.canChooseSandbox {
                Button("Choose sandbox") {
                    actions.onChooseSandbox?()
                }
                .controlSize(.small)
            }
        }
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
