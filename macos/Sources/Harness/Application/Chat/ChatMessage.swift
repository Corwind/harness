import Foundation

public typealias ConversationId = String

public enum ChatToolStatus: Sendable, Equatable {
    case streamingInput
    case executing
    case finished
    case errored
}

public struct ChatToolCall: Identifiable, Sendable, Equatable {
    public let id: String
    public var name: String
    public var partialJSON: String
    public var input: [String: JSONValue]?
    public var status: ChatToolStatus
    public var stdout: String
    public var stderr: String
    public var output: JSONValue?
    public var exitCode: Int?
    public var durationMs: Int?
    public var errorCode: String?
    public var errorMessage: String?
    public var sandboxTemplateId: String?
    public var kind: ToolKind?

    public init(
        id: String,
        name: String,
        partialJSON: String = "",
        input: [String: JSONValue]? = nil,
        status: ChatToolStatus = .streamingInput,
        stdout: String = "",
        stderr: String = "",
        output: JSONValue? = nil,
        exitCode: Int? = nil,
        durationMs: Int? = nil,
        errorCode: String? = nil,
        errorMessage: String? = nil,
        sandboxTemplateId: String? = nil,
        kind: ToolKind? = nil
    ) {
        self.id = id
        self.name = name
        self.partialJSON = partialJSON
        self.input = input
        self.status = status
        self.stdout = stdout
        self.stderr = stderr
        self.output = output
        self.exitCode = exitCode
        self.durationMs = durationMs
        self.errorCode = errorCode
        self.errorMessage = errorMessage
        self.sandboxTemplateId = sandboxTemplateId
        self.kind = kind
    }
}

public enum ChatMessageStatus: Sendable, Equatable {
    case sending
    case sent
    case streaming
    case complete
    case cancelled
    case errored
}

public struct ChatMessage: Identifiable, Sendable, Equatable {
    public let id: String
    public let role: MessageRole
    public var text: String
    public var toolCalls: [ChatToolCall]
    public var status: ChatMessageStatus

    public init(
        id: String,
        role: MessageRole,
        text: String = "",
        toolCalls: [ChatToolCall] = [],
        status: ChatMessageStatus = .complete
    ) {
        self.id = id
        self.role = role
        self.text = text
        self.toolCalls = toolCalls
        self.status = status
    }
}

extension ChatMessage {
    /// Hydrate a UI message from a persisted Domain.Message (used for history reload).
    public static func fromDomain(_ m: Message) -> ChatMessage {
        var text = ""
        var toolCalls: [ChatToolCall] = []
        for block in m.content {
            switch block {
            case .text(let t):
                if !text.isEmpty { text.append("\n") }
                text.append(t.text)
            case .toolUse(let tu):
                toolCalls.append(ChatToolCall(
                    id: tu.id,
                    name: tu.name,
                    input: tu.input,
                    status: .finished
                ))
            case .toolResult(let tr):
                if let idx = toolCalls.firstIndex(where: { $0.id == tr.toolUseId }) {
                    toolCalls[idx].output = tr.output
                    toolCalls[idx].status = tr.isError ? .errored : .finished
                }
            case .image:
                break
            }
        }
        return ChatMessage(
            id: m.id,
            role: m.role,
            text: text,
            toolCalls: toolCalls,
            status: .complete
        )
    }
}
