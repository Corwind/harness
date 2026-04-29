import Foundation

/// Tracing level reported by the backend's diagnostics endpoint.
public enum LogLevel: String, Codable, Sendable, Equatable, CaseIterable {
    case error
    case warn
    case info
    case debug
    case trace

    /// Unknown levels sent by future backends decode to `.info` so the
    /// log tail keeps rendering rather than crashing on a single bad row.
    public init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = LogLevel(rawValue: raw.lowercased()) ?? .info
    }
}

/// One captured log record from `GET /v1/diagnostics/logs`.
public struct LogLine: Codable, Sendable, Equatable, Identifiable {
    public let seq: Int
    public let level: LogLevel
    /// RFC 3339 timestamp; opaque to the UI (rendered verbatim).
    public let ts: String
    public let target: String
    public let message: String

    public var id: Int { seq }

    public init(seq: Int, level: LogLevel, ts: String, target: String, message: String) {
        self.seq = seq
        self.level = level
        self.ts = ts
        self.target = target
        self.message = message
    }
}

/// One page returned by the diagnostics endpoint. `nextSeq` is the cursor
/// the client echoes on the next poll (stable across empty windows).
public struct LogPage: Sendable, Equatable {
    public let logs: [LogLine]
    public let nextSeq: Int

    public init(logs: [LogLine], nextSeq: Int) {
        self.logs = logs
        self.nextSeq = nextSeq
    }
}
