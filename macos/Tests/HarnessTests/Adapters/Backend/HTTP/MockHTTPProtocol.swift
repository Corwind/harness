import Foundation
@testable import HarnessApp

/// `URLProtocol` subclass that intercepts every request issued through a
/// matching `URLSession` and serves canned responses from a thread-safe
/// in-memory router.
final class MockHTTPProtocol: URLProtocol, @unchecked Sendable {

    struct Response {
        var status: Int
        var headers: [String: String]
        var bodyChunks: [Data]
        /// Sleep before each chunk (seconds). Defaults to 0.
        var delayBetweenChunks: TimeInterval

        init(status: Int = 200, headers: [String: String] = [:], body: Data = Data(), bodyChunks: [Data]? = nil, delayBetweenChunks: TimeInterval = 0) {
            self.status = status
            self.headers = headers
            self.bodyChunks = bodyChunks ?? [body]
            self.delayBetweenChunks = delayBetweenChunks
        }
    }

    typealias Handler = @Sendable (URLRequest) -> Response

    final class Router: @unchecked Sendable {
        private let lock = NSLock()
        private var handlers: [Handler] = []
        private(set) var observed: [URLRequest] = []
        // Tracks request paths whose URLSessionDataTask was cancelled mid-stream.
        private(set) var cancelledPaths: [String] = []
        // Tracks URLProtocol stopLoading calls (URLSession-side cancellation signal).
        private(set) var stoppedPaths: [String] = []

        func register(_ handler: @escaping Handler) {
            lock.lock(); defer { lock.unlock() }
            handlers.append(handler)
        }

        func record(_ request: URLRequest) {
            lock.lock(); defer { lock.unlock() }
            observed.append(request)
        }

        func recordCancellation(_ path: String) {
            lock.lock(); defer { lock.unlock() }
            cancelledPaths.append(path)
        }

        func recordStop(_ path: String) {
            lock.lock(); defer { lock.unlock() }
            stoppedPaths.append(path)
        }

        func resolve(_ request: URLRequest) -> Response {
            lock.lock()
            let snapshot = handlers
            lock.unlock()
            for handler in snapshot {
                return handler(request)
            }
            return Response(status: 599, body: Data("no handler".utf8))
        }
    }

    private static let routerLock = NSLock()
    private static var router: Router?

    static func install(router: Router) {
        routerLock.lock(); defer { routerLock.unlock() }
        self.router = router
        URLProtocol.registerClass(MockHTTPProtocol.self)
    }

    static func reset() {
        routerLock.lock(); defer { routerLock.unlock() }
        URLProtocol.unregisterClass(MockHTTPProtocol.self)
        self.router = nil
    }

    static func current() -> Router? {
        routerLock.lock(); defer { routerLock.unlock() }
        return router
    }

    static func makeSession() -> URLSession {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockHTTPProtocol.self]
        return URLSession(configuration: config)
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    private var streamingTask: Task<Void, Never>?

    override func startLoading() {
        guard let router = MockHTTPProtocol.current() else {
            client?.urlProtocol(self, didFailWithError: URLError(.unknown))
            return
        }
        router.record(request)
        let response = router.resolve(request)

        let url = request.url ?? URL(string: "http://invalid")!
        var headers = response.headers
        if headers["Content-Type"] == nil {
            headers["Content-Type"] = "application/json"
        }
        let httpResponse = HTTPURLResponse(
            url: url,
            statusCode: response.status,
            httpVersion: "HTTP/1.1",
            headerFields: headers
        )!
        client?.urlProtocol(self, didReceive: httpResponse, cacheStoragePolicy: .notAllowed)

        let chunks = response.bodyChunks
        let delay = response.delayBetweenChunks
        let pathForCancel = request.url?.path ?? ""
        let weakClient = self.client
        let proto = self
        streamingTask = Task.detached { [weak proto] in
            for chunk in chunks {
                if Task.isCancelled {
                    router.recordCancellation(pathForCancel)
                    return
                }
                if delay > 0 {
                    try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000))
                    if Task.isCancelled {
                        router.recordCancellation(pathForCancel)
                        return
                    }
                }
                guard let proto else { return }
                weakClient?.urlProtocol(proto, didLoad: chunk)
            }
            if let proto {
                weakClient?.urlProtocolDidFinishLoading(proto)
            }
        }
    }

    override func stopLoading() {
        if let router = MockHTTPProtocol.current() {
            router.recordStop(request.url?.path ?? "")
        }
        streamingTask?.cancel()
        streamingTask = nil
    }
}
