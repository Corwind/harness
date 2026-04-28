import Foundation

/// Stop reason mirrors `harness_core::StopReason` (see spec/events.md).
public enum StopReason: String, Codable, Sendable, Equatable {
    case endTurn = "end_turn"
    case maxTokens = "max_tokens"
    case stopSequence = "stop_sequence"
    case toolUse = "tool_use"
    case cancelled
    case other
}

public struct Usage: Codable, Sendable, Equatable {
    public let inputTokens: Int?
    public let outputTokens: Int?

    public init(inputTokens: Int? = nil, outputTokens: Int? = nil) {
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
    }

    enum CodingKeys: String, CodingKey {
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
    }
}

public enum RunStatus: String, Codable, Sendable, Equatable {
    case completed
    case errored
    case cancelled
}

public enum ToolKind: String, Codable, Sendable, Equatable {
    case external
    case inProcess = "in_process"
}

public struct ToolFinishOutput: Codable, Sendable, Equatable {
    public let raw: JSONValue

    public init(raw: JSONValue) { self.raw = raw }

    public init(from decoder: Decoder) throws {
        self.raw = try JSONValue(from: decoder)
    }

    public func encode(to encoder: Encoder) throws {
        try raw.encode(to: encoder)
    }
}

/// Typed SSE events from `/v1/runs/{run_id}/events`.
/// Names mirror `spec/events.md`.
public enum RunEvent: Sendable, Equatable {
    case runStart(RunStartPayload)
    case messageStart(MessageStartPayload)
    case contentDelta(ContentDeltaPayload)
    case toolUseStart(ToolUseStartPayload)
    case toolUseDelta(ToolUseDeltaPayload)
    case toolUseStop(ToolUseStopPayload)
    case toolStart(ToolStartPayload)
    case toolStdout(ToolChunkPayload)
    case toolStderr(ToolChunkPayload)
    case toolFinish(ToolFinishPayload)
    case toolError(ToolErrorPayload)
    case messageStop(MessageStopPayload)
    case error(ErrorPayload)
    case runEnd(RunEndPayload)
}

public struct RunStartPayload: Codable, Sendable, Equatable {
    public let runId: String
    public let conversationId: String
    public let startedAt: String

    public init(runId: String, conversationId: String, startedAt: String) {
        self.runId = runId
        self.conversationId = conversationId
        self.startedAt = startedAt
    }

    enum CodingKeys: String, CodingKey {
        case runId = "run_id"
        case conversationId = "conversation_id"
        case startedAt = "started_at"
    }
}

public struct MessageStartPayload: Codable, Sendable, Equatable {
    public let id: String
    public init(id: String) { self.id = id }
}

public struct ContentDeltaPayload: Codable, Sendable, Equatable {
    public let text: String
    public init(text: String) { self.text = text }
}

public struct ToolUseStartPayload: Codable, Sendable, Equatable {
    public let id: String
    public let name: String
    public init(id: String, name: String) { self.id = id; self.name = name }
}

public struct ToolUseDeltaPayload: Codable, Sendable, Equatable {
    public let id: String
    public let partialJSON: String

    public init(id: String, partialJSON: String) {
        self.id = id
        self.partialJSON = partialJSON
    }

    enum CodingKeys: String, CodingKey {
        case id
        case partialJSON = "partial_json"
    }
}

public struct ToolUseStopPayload: Codable, Sendable, Equatable {
    public let id: String
    public let input: [String: JSONValue]

    public init(id: String, input: [String: JSONValue]) {
        self.id = id
        self.input = input
    }
}

public struct ToolStartPayload: Codable, Sendable, Equatable {
    public let toolUseId: String
    public let name: String
    public let kind: ToolKind
    public let sandboxTemplateId: String?

    public init(toolUseId: String, name: String, kind: ToolKind, sandboxTemplateId: String?) {
        self.toolUseId = toolUseId
        self.name = name
        self.kind = kind
        self.sandboxTemplateId = sandboxTemplateId
    }

    enum CodingKeys: String, CodingKey {
        case toolUseId = "tool_use_id"
        case name
        case kind
        case sandboxTemplateId = "sandbox_template_id"
    }
}

public struct ToolChunkPayload: Codable, Sendable, Equatable {
    public let toolUseId: String
    public let chunk: String

    public init(toolUseId: String, chunk: String) {
        self.toolUseId = toolUseId
        self.chunk = chunk
    }

    enum CodingKeys: String, CodingKey {
        case toolUseId = "tool_use_id"
        case chunk
    }
}

public struct ToolFinishPayload: Codable, Sendable, Equatable {
    public let toolUseId: String
    public let output: JSONValue
    public let exitCode: Int?
    public let durationMs: Int

    public init(toolUseId: String, output: JSONValue, exitCode: Int?, durationMs: Int) {
        self.toolUseId = toolUseId
        self.output = output
        self.exitCode = exitCode
        self.durationMs = durationMs
    }

    enum CodingKeys: String, CodingKey {
        case toolUseId = "tool_use_id"
        case output
        case exitCode = "exit_code"
        case durationMs = "duration_ms"
    }
}

public struct ToolErrorPayload: Codable, Sendable, Equatable {
    public let toolUseId: String
    public let code: String
    public let message: String

    public init(toolUseId: String, code: String, message: String) {
        self.toolUseId = toolUseId
        self.code = code
        self.message = message
    }

    enum CodingKeys: String, CodingKey {
        case toolUseId = "tool_use_id"
        case code
        case message
    }
}

public struct MessageStopPayload: Codable, Sendable, Equatable {
    public let stopReason: StopReason
    public let usage: Usage?

    public init(stopReason: StopReason, usage: Usage? = nil) {
        self.stopReason = stopReason
        self.usage = usage
    }

    enum CodingKeys: String, CodingKey {
        case stopReason = "stop_reason"
        case usage
    }
}

public struct ErrorPayload: Codable, Sendable, Equatable {
    public let code: String?
    public let message: String

    public init(code: String? = nil, message: String) {
        self.code = code
        self.message = message
    }
}

public struct RunEndPayload: Codable, Sendable, Equatable {
    public let runId: String
    public let status: RunStatus
    public let endedAt: String

    public init(runId: String, status: RunStatus, endedAt: String) {
        self.runId = runId
        self.status = status
        self.endedAt = endedAt
    }

    enum CodingKeys: String, CodingKey {
        case runId = "run_id"
        case status
        case endedAt = "ended_at"
    }
}
