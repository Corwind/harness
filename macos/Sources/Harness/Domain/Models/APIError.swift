import Foundation

/// RFC 7807 problem+json body the backend returns for failures.
public struct APIErrorBody: Codable, Sendable, Equatable {
    public let type: String
    public let title: String
    public let status: Int
    public let detail: String?
    public let instance: String?
    public let code: String?

    public init(type: String, title: String, status: Int, detail: String? = nil, instance: String? = nil, code: String? = nil) {
        self.type = type
        self.title = title
        self.status = status
        self.detail = detail
        self.instance = instance
        self.code = code
    }
}

/// Errors surfaced by the HTTP backend adapter.
public enum BackendError: Error, Sendable, Equatable {
    /// Server returned an HTTP error status (>= 400). Body is parsed when available.
    case httpStatus(Int, body: APIErrorBody?)
    /// Body decode failed for the expected type.
    case decoding(String)
    /// Body encode failed for the request payload.
    case encoding(String)
    /// Transport-level failure (URLSession error, broken stream, etc.).
    case transport(String)
    /// Server returned a non-JSON body where one was expected.
    case malformedResponse(String)
    /// SSE stream produced an invalid frame.
    case malformedEvent(String)
    /// Stream was cancelled by the client.
    case cancelled
}
