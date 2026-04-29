import Foundation

@MainActor
@Observable
public final class ChatViewModel {
    public private(set) var messages: [ChatMessage] = []
    public private(set) var isStreaming: Bool = false
    public private(set) var isLoadingHistory: Bool = false
    public private(set) var didLoadHistoryOnce: Bool = false
    public private(set) var error: ChatError? = nil
    public private(set) var runStatus: RunStatus? = nil

    private let runGateway: RunGateway
    private let messageGateway: MessageGateway
    private let conversationId: ConversationId

    private var currentRunId: String? = nil
    private var currentAssistantMessageIndex: Int? = nil
    private var streamingTask: Task<Void, Never>? = nil
    private var lastUserMessageText: String? = nil

    public init(
        runGateway: RunGateway,
        messageGateway: MessageGateway,
        conversationId: ConversationId
    ) {
        self.runGateway = runGateway
        self.messageGateway = messageGateway
        self.conversationId = conversationId
    }

    /// `true` once history has been fetched and the conversation has no
    /// messages — drives the "Start the conversation." empty state.
    public var isEmpty: Bool {
        didLoadHistoryOnce && messages.isEmpty && !isLoadingHistory && !isStreaming
    }

    public func loadHistory() async {
        isLoadingHistory = true
        defer {
            isLoadingHistory = false
            didLoadHistoryOnce = true
        }
        do {
            let domainMessages = try await messageGateway.list(
                conversationId: conversationId,
                limit: nil,
                afterOrdinal: nil
            )
            self.messages = domainMessages.map(ChatMessage.fromDomain)
        } catch {
            self.error = ChatError.from(error)
        }
    }

    /// Clear the surfaced error so the view can dismiss its banner.
    public func clearError() {
        error = nil
    }

    /// Retry the last user message after a transient failure. No-op if
    /// no prior message has been sent in this session.
    public func retry() async {
        guard let text = lastUserMessageText else { return }
        await send(text)
    }

    public func send(_ text: String) async {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, !isStreaming else { return }

        error = nil
        runStatus = nil
        lastUserMessageText = trimmed

        let userMessage = ChatMessage(
            id: "local-user-\(UUID().uuidString)",
            role: .user,
            text: trimmed,
            status: .sent
        )
        messages.append(userMessage)

        isStreaming = true

        let request = PostMessageRequest(
            content: [.text(TextBlock(text: trimmed))]
        )

        let handle: RunHandle
        do {
            handle = try await messageGateway.post(
                conversationId: conversationId,
                request
            )
        } catch {
            self.error = ChatError.from(error)
            self.isStreaming = false
            if let idx = messages.lastIndex(where: { $0.id == userMessage.id }) {
                messages[idx].status = .errored
            }
            return
        }

        currentRunId = handle.runId
        let runId = handle.runId

        streamingTask = Task { [weak self] in
            await self?.consumeEvents(runId: runId)
        }
        await streamingTask?.value
    }

    public func cancel() {
        guard isStreaming else { return }
        let runId = currentRunId
        streamingTask?.cancel()
        streamingTask = nil

        if let runId {
            Task { [runGateway] in
                try? await runGateway.cancel(runId: runId)
            }
        }

        finalizeStream(status: .cancelled)
    }

    private func consumeEvents(runId: String) async {
        let stream: AsyncThrowingStream<RunEvent, Error>
        do {
            stream = try await runGateway.events(runId: runId, lastEventId: nil)
        } catch {
            self.error = ChatError.from(error)
            finalizeStream(status: .errored)
            return
        }

        do {
            for try await event in stream {
                if Task.isCancelled { break }
                handle(event)
            }
        } catch is CancellationError {
        } catch {
            if !Task.isCancelled {
                self.error = ChatError.from(error)
                finalizeStream(status: .errored)
            }
        }
    }

    private func handle(_ event: RunEvent) {
        switch event {
        case .runStart:
            break

        case .messageStart(let payload):
            let assistant = ChatMessage(
                id: payload.id,
                role: .assistant,
                text: "",
                toolCalls: [],
                status: .streaming
            )
            messages.append(assistant)
            currentAssistantMessageIndex = messages.count - 1

        case .contentDelta(let payload):
            ensureAssistantMessage()
            guard let idx = currentAssistantMessageIndex else { return }
            messages[idx].text.append(payload.text)

        case .toolUseStart(let payload):
            ensureAssistantMessage()
            guard let idx = currentAssistantMessageIndex else { return }
            let call = ChatToolCall(
                id: payload.id,
                name: payload.name,
                status: .streamingInput
            )
            messages[idx].toolCalls.append(call)

        case .toolUseDelta(let payload):
            mutateToolCall(id: payload.id) { call in
                call.partialJSON.append(payload.partialJSON)
            }

        case .toolUseStop(let payload):
            mutateToolCall(id: payload.id) { call in
                call.input = payload.input
            }

        case .toolStart(let payload):
            mutateToolCall(id: payload.toolUseId) { call in
                call.status = .executing
                call.kind = payload.kind
                call.sandboxTemplateId = payload.sandboxTemplateId
            }

        case .toolStdout(let payload):
            mutateToolCall(id: payload.toolUseId) { call in
                call.stdout.append(payload.chunk)
            }

        case .toolStderr(let payload):
            mutateToolCall(id: payload.toolUseId) { call in
                call.stderr.append(payload.chunk)
            }

        case .toolFinish(let payload):
            mutateToolCall(id: payload.toolUseId) { call in
                call.status = .finished
                call.output = payload.output
                call.exitCode = payload.exitCode
                call.durationMs = payload.durationMs
            }

        case .toolError(let payload):
            mutateToolCall(id: payload.toolUseId) { call in
                call.status = .errored
                call.errorCode = payload.code
                call.errorMessage = payload.message
            }

        case .messageStop:
            if let idx = currentAssistantMessageIndex {
                messages[idx].status = .complete
            }
            currentAssistantMessageIndex = nil

        case .error(let payload):
            self.error = ChatError.from(payload)
            if let idx = currentAssistantMessageIndex {
                messages[idx].status = .errored
            }

        case .runEnd(let payload):
            finalizeStream(status: payload.status)
        }
    }

    private func ensureAssistantMessage() {
        if currentAssistantMessageIndex == nil {
            let assistant = ChatMessage(
                id: "assistant-\(UUID().uuidString)",
                role: .assistant,
                text: "",
                toolCalls: [],
                status: .streaming
            )
            messages.append(assistant)
            currentAssistantMessageIndex = messages.count - 1
        }
    }

    private func mutateToolCall(id: String, _ block: (inout ChatToolCall) -> Void) {
        guard let msgIdx = currentAssistantMessageIndex else { return }
        guard let toolIdx = messages[msgIdx].toolCalls.firstIndex(where: { $0.id == id }) else { return }
        block(&messages[msgIdx].toolCalls[toolIdx])
    }

    private func finalizeStream(status: RunStatus) {
        runStatus = status
        isStreaming = false
        if let idx = currentAssistantMessageIndex {
            switch status {
            case .completed:
                messages[idx].status = .complete
            case .cancelled:
                messages[idx].status = .cancelled
            case .errored:
                messages[idx].status = .errored
            }
        }
        currentAssistantMessageIndex = nil
        currentRunId = nil
        streamingTask = nil
    }
}
