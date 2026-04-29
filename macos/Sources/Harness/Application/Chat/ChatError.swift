import Foundation

/// Structured classification of every failure mode the chat surface can
/// hit. Drives both the message string and the actionable buttons the UI
/// shows ("Open Settings", "Choose sandbox", "Retry"). New cases here
/// must map a stable `code` (server-side) or `BackendError` variant to a
/// user-readable message — the view layer never inspects raw errors.
public enum ChatErrorKind: Sendable, Equatable {
    /// `BackendError.transport`: backend offline / dropped connection.
    case transport
    /// HTTP 401 from the backend itself (not the LLM provider). The
    /// sidecar handshake mints a fresh token at startup, so this means
    /// the session became invalid mid-run; treat as fatal.
    case sessionExpired
    /// HTTP 403 — typically a non-loopback origin rejection.
    case forbidden
    /// `provider.unauthorized` — invalid API key.
    case providerUnauthorized
    /// `provider.rate_limited` — optional retry-after seconds.
    case providerRateLimited(retryAfterSeconds: Int?)
    /// `provider.unconfigured` — provider has no API key on file.
    case providerUnconfigured
    /// `provider.unavailable` / `provider.upstream` — transient provider
    /// failure; show retry.
    case providerUnavailable
    /// `sandbox.required` — conversation has no template attached.
    case sandboxRequired
    /// `sandbox.invalid_profile` — a saved template failed validation.
    case sandboxInvalidProfile(templateId: String?)
    /// `tool.timeout` — tool execution exceeded its budget.
    case toolTimeout
    /// `tool.spawn_failed` — could not spawn the sandboxed subprocess.
    case toolSpawnFailed
    /// Catch-all for HTTP statuses or codes we don't classify.
    case server(status: Int?, detail: String?)
    /// Anything else (decoding failures, malformed events, ad-hoc).
    case other
}

/// Suggested actions the UI can surface for a given error. Multiple
/// actions can apply (a transport error allows retry; a provider-
/// unauthorized error surfaces both a Settings link and retry).
public struct ChatErrorActions: Sendable, Equatable {
    public var canRetry: Bool
    public var canOpenSettings: Bool
    public var canChooseSandbox: Bool
    public var settingsTab: SettingsTab?

    public enum SettingsTab: Sendable, Equatable {
        case providers
        case sandboxes(templateId: String?)
    }

    public static let none = ChatErrorActions(
        canRetry: false,
        canOpenSettings: false,
        canChooseSandbox: false,
        settingsTab: nil
    )
}

public struct ChatError: Sendable, Equatable, Error {
    public let kind: ChatErrorKind
    public let code: String?
    public let message: String
    public let actions: ChatErrorActions

    public init(
        kind: ChatErrorKind = .other,
        code: String? = nil,
        message: String,
        actions: ChatErrorActions = .none
    ) {
        self.kind = kind
        self.code = code
        self.message = message
        self.actions = actions
    }

    public static func from(_ error: Error) -> ChatError {
        if let chatError = error as? ChatError { return chatError }
        if let backend = error as? BackendError { return classify(backend) }
        return ChatError(
            kind: .other,
            code: nil,
            message: String(describing: error),
            actions: .none
        )
    }

    public static func from(_ payload: ErrorPayload) -> ChatError {
        classify(code: payload.code, message: payload.message)
    }

    /// Classify a server-emitted SSE `error` event by its stable `code`.
    /// The `code` taxonomy mirrors `harness_core::ProviderError` and
    /// `ToolError`; new codes that ship server-side default to `.other`
    /// with the raw message until added here.
    public static func classify(code: String?, message: String) -> ChatError {
        switch code {
        case "provider.unauthorized":
            return ChatError(
                kind: .providerUnauthorized,
                code: code,
                message: "API key invalid — open Settings → Providers.",
                actions: ChatErrorActions(
                    canRetry: false,
                    canOpenSettings: true,
                    canChooseSandbox: false,
                    settingsTab: .providers
                )
            )
        case "provider.rate_limited":
            let retryAfter = parseRetryAfter(from: message)
            let pretty: String
            if let retryAfter {
                pretty = "Rate-limited by provider — retrying in \(retryAfter)s."
            } else {
                pretty = "Rate-limited by provider — try again in a moment."
            }
            return ChatError(
                kind: .providerRateLimited(retryAfterSeconds: retryAfter),
                code: code,
                message: pretty,
                actions: ChatErrorActions(
                    canRetry: true,
                    canOpenSettings: false,
                    canChooseSandbox: false,
                    settingsTab: nil
                )
            )
        case "provider.unconfigured":
            return ChatError(
                kind: .providerUnconfigured,
                code: code,
                message: "Provider not configured — open Settings → Providers to add an API key.",
                actions: ChatErrorActions(
                    canRetry: false,
                    canOpenSettings: true,
                    canChooseSandbox: false,
                    settingsTab: .providers
                )
            )
        case "provider.unavailable", "provider.upstream":
            return ChatError(
                kind: .providerUnavailable,
                code: code,
                message: "Provider temporarily unavailable. Try again.",
                actions: ChatErrorActions(
                    canRetry: true,
                    canOpenSettings: false,
                    canChooseSandbox: false,
                    settingsTab: nil
                )
            )
        case "sandbox.required":
            return ChatError(
                kind: .sandboxRequired,
                code: code,
                message: "Tools blocked — this conversation has no sandbox attached.",
                actions: ChatErrorActions(
                    canRetry: false,
                    canOpenSettings: false,
                    canChooseSandbox: true,
                    settingsTab: nil
                )
            )
        case "sandbox.invalid_profile":
            return ChatError(
                kind: .sandboxInvalidProfile(templateId: nil),
                code: code,
                message: "Sandbox profile failed validation — open Settings → Sandboxes.",
                actions: ChatErrorActions(
                    canRetry: false,
                    canOpenSettings: true,
                    canChooseSandbox: false,
                    settingsTab: .sandboxes(templateId: nil)
                )
            )
        case "tool.timeout":
            return ChatError(
                kind: .toolTimeout,
                code: code,
                message: "Tool execution timed out.",
                actions: ChatErrorActions(
                    canRetry: true,
                    canOpenSettings: false,
                    canChooseSandbox: false,
                    settingsTab: nil
                )
            )
        case "tool.spawn_failed":
            return ChatError(
                kind: .toolSpawnFailed,
                code: code,
                message: "Couldn't start the tool subprocess: \(message)",
                actions: .none
            )
        default:
            return ChatError(
                kind: .other,
                code: code,
                message: message,
                actions: .none
            )
        }
    }

    private static func classify(_ error: BackendError) -> ChatError {
        switch error {
        case .transport(let detail):
            return ChatError(
                kind: .transport,
                code: nil,
                message: "Backend offline. Retrying… (\(detail))",
                actions: ChatErrorActions(
                    canRetry: true,
                    canOpenSettings: false,
                    canChooseSandbox: false,
                    settingsTab: nil
                )
            )
        case .httpStatus(let status, let body):
            // Honour a body.code if it carries a typed provider/tool
            // taxonomy (e.g. `provider.unauthorized` arriving as a 401
            // body code). The response status alone can't disambiguate
            // between "this Harness session expired" and "the upstream
            // provider rejected the api key" — only the code can.
            if let bodyCode = body?.code {
                let bodyMessage = body?.detail ?? body?.title ?? "Backend error \(status)."
                let classified = classify(code: bodyCode, message: bodyMessage)
                if case .other = classified.kind {
                    // Fall through to status-based fallbacks.
                } else {
                    return classified
                }
            }
            switch status {
            case 401:
                return ChatError(
                    kind: .sessionExpired,
                    code: body?.code ?? "401",
                    message: "Backend session expired. Relaunch Harness to recover.",
                    actions: .none
                )
            case 403:
                return ChatError(
                    kind: .forbidden,
                    code: body?.code ?? "403",
                    message: "Forbidden — the backend rejected this origin.",
                    actions: .none
                )
            default:
                return ChatError(
                    kind: .server(status: status, detail: body?.detail),
                    code: body?.code,
                    message: body?.detail ?? "Backend error \(status).",
                    actions: .none
                )
            }
        case .decoding(let detail):
            return ChatError(
                kind: .other,
                code: nil,
                message: "Couldn't read the backend response: \(detail)",
                actions: .none
            )
        case .encoding(let detail):
            return ChatError(
                kind: .other,
                code: nil,
                message: "Couldn't send the request: \(detail)",
                actions: .none
            )
        case .malformedResponse(let detail):
            return ChatError(
                kind: .other,
                code: nil,
                message: "Backend returned a malformed response: \(detail)",
                actions: .none
            )
        case .malformedEvent(let detail):
            return ChatError(
                kind: .other,
                code: nil,
                message: "Stream produced an invalid event: \(detail)",
                actions: .none
            )
        case .cancelled:
            return ChatError(
                kind: .other,
                code: nil,
                message: "Cancelled.",
                actions: .none
            )
        }
    }

    /// Extract a numeric retry-after value embedded in the provider's
    /// rate-limit message (e.g. "retry after 30s" or
    /// "retry-after: 30"). Returns `nil` if no integer is present.
    private static func parseRetryAfter(from message: String) -> Int? {
        let pattern = #"(\d+)\s*s"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return nil }
        let range = NSRange(message.startIndex..., in: message)
        if let match = regex.firstMatch(in: message, range: range),
           match.numberOfRanges >= 2,
           let intRange = Range(match.range(at: 1), in: message) {
            return Int(message[intRange])
        }
        return nil
    }
}
