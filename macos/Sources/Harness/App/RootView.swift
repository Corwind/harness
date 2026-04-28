import SwiftUI

/// The window root once the backend session has been acquired. Builds the
/// HTTP client + gateways, hands them to a `ChatRootViewModel`, and renders
/// the chat surface.
struct RootView: View {
    let session: BackendSession
    @State private var rootVM: ChatRootViewModel

    init(session: BackendSession) {
        self.session = session
        let client = HTTPClient(baseURL: session.baseURL, token: session.token)
        let runGateway = RunGatewayAdapter(client: client)
        let messageGateway = MessageGatewayAdapter(client: client)
        let conversationGateway = ConversationGatewayAdapter(client: client)
        let providersGateway = ProvidersGatewayAdapter(client: client)
        _rootVM = State(initialValue: ChatRootViewModel(
            runGateway: runGateway,
            messageGateway: messageGateway,
            conversationGateway: conversationGateway,
            providersGateway: providersGateway
        ))
    }

    var body: some View {
        Group {
            switch rootVM.state {
            case .loading:
                VStack(spacing: 12) {
                    ProgressView()
                    Text("Preparing your conversation…")
                        .foregroundStyle(.secondary)
                }
                .frame(minWidth: 480, minHeight: 360)
            case .ready(let chatVM):
                ChatView(viewModel: chatVM)
            case .failed(let error):
                FailureView(error: error) {
                    Task { await rootVM.bootstrap() }
                }
            }
        }
        .task {
            if case .loading = rootVM.state {
                await rootVM.bootstrap()
            }
        }
    }
}

private struct FailureView: View {
    let error: ChatError
    let retry: () -> Void

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.largeTitle)
                .foregroundStyle(.red)
            Text("Couldn't open a conversation")
                .font(.headline)
            Text(error.message)
                .font(.system(.caption, design: .monospaced))
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
            Button("Try again", action: retry)
                .keyboardShortcut("r", modifiers: .command)
        }
        .padding(24)
        .frame(minWidth: 480, minHeight: 360)
    }
}
