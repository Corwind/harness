import Foundation

/// Thin URLSession wrapper that injects the X-Harness-Token header on every
/// request and decodes JSON responses. RFC 7807 problem+json error bodies are
/// surfaced as `BackendError.httpStatus`.
public final class HTTPClient: @unchecked Sendable {
    public typealias SessionFactory = @Sendable () -> URLSession

    public let baseURL: URL
    public let token: String
    private let session: URLSession
    private let encoder: JSONEncoder
    private let decoder: JSONDecoder

    public init(
        baseURL: URL,
        token: String,
        session: URLSession = .shared
    ) {
        self.baseURL = baseURL
        self.token = token
        self.session = session
        self.encoder = JSONEncoder()
        self.decoder = JSONDecoder()
    }

    public var underlyingSession: URLSession { session }

    public func makeRequest(
        method: String,
        path: String,
        query: [URLQueryItem] = [],
        headers: [String: String] = [:],
        body: Data? = nil
    ) -> URLRequest {
        var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: false)!
        let basePath = components.path
        components.path = basePath + path
        if !query.isEmpty {
            components.queryItems = query
        }
        var request = URLRequest(url: components.url!)
        request.httpMethod = method
        request.setValue(token, forHTTPHeaderField: "X-Harness-Token")
        if body != nil {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        for (k, v) in headers {
            request.setValue(v, forHTTPHeaderField: k)
        }
        request.httpBody = body
        return request
    }

    public func send<Response: Decodable>(
        _ request: URLRequest,
        as: Response.Type
    ) async throws -> Response {
        let (data, response) = try await dataTask(for: request)
        try validateStatus(response: response, data: data)
        do {
            return try decoder.decode(Response.self, from: data)
        } catch {
            throw BackendError.decoding(String(describing: error))
        }
    }

    public func sendNoContent(_ request: URLRequest) async throws {
        let (data, response) = try await dataTask(for: request)
        try validateStatus(response: response, data: data)
    }

    public func encode<T: Encodable>(_ value: T) throws -> Data {
        do {
            return try encoder.encode(value)
        } catch {
            throw BackendError.encoding(String(describing: error))
        }
    }

    private func dataTask(for request: URLRequest) async throws -> (Data, URLResponse) {
        do {
            return try await session.data(for: request)
        } catch is CancellationError {
            throw BackendError.cancelled
        } catch let urlError as URLError where urlError.code == .cancelled {
            throw BackendError.cancelled
        } catch {
            throw BackendError.transport(String(describing: error))
        }
    }

    private func validateStatus(response: URLResponse, data: Data) throws {
        guard let http = response as? HTTPURLResponse else {
            throw BackendError.malformedResponse("Non-HTTP response")
        }
        if (200..<300).contains(http.statusCode) { return }
        let body = try? decoder.decode(APIErrorBody.self, from: data)
        throw BackendError.httpStatus(http.statusCode, body: body)
    }
}
