import XCTest
@testable import HarnessApp

final class HTTPClientTests: XCTestCase {

    private var router: MockHTTPProtocol.Router!
    private var session: URLSession!

    override func setUp() {
        super.setUp()
        router = MockHTTPProtocol.Router()
        MockHTTPProtocol.install(router: router)
        session = MockHTTPProtocol.makeSession()
    }

    override func tearDown() {
        MockHTTPProtocol.reset()
        session = nil
        router = nil
        super.tearDown()
    }

    // 1. GET /v1/health → parsed Health value with token; 401 on bad/missing token.
    func test_health_returnsHealth_andSendsAuthHeader() async throws {
        router.register { request in
            // Backend would normally guard this — here we surface what the client sent
            // through the route handler; auth header presence is asserted below.
            let body = #"{"status":"ok","version":"0.1.0"}"#.data(using: .utf8)!
            return .init(status: 200, body: body)
        }
        let client = HTTPClient(
            baseURL: URL(string: "http://127.0.0.1:8080")!,
            token: "tok-abc",
            session: session
        )
        let gateway = HealthGatewayAdapter(client: client)

        let health = try await gateway.getHealth()
        XCTAssertEqual(health, Health(status: "ok", version: "0.1.0"))

        XCTAssertEqual(router.observed.count, 1)
        XCTAssertEqual(router.observed[0].url?.path, "/v1/health")
        XCTAssertEqual(router.observed[0].value(forHTTPHeaderField: "X-Harness-Token"), "tok-abc")
    }

    func test_health_unauthorized_throwsHttpStatus401() async {
        router.register { _ in
            let body = #"{"type":"about:blank","title":"Unauthorized","status":401}"#.data(using: .utf8)!
            return .init(
                status: 401,
                headers: ["Content-Type": "application/problem+json"],
                body: body
            )
        }
        let client = HTTPClient(
            baseURL: URL(string: "http://127.0.0.1:8080")!,
            token: "bad",
            session: session
        )
        let gateway = HealthGatewayAdapter(client: client)

        do {
            _ = try await gateway.getHealth()
            XCTFail("Expected error")
        } catch let BackendError.httpStatus(code, body) {
            XCTAssertEqual(code, 401)
            XCTAssertEqual(body?.title, "Unauthorized")
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    // 6. Auth header injected on every request — verified across multiple endpoints.
    func test_authHeaderIsInjectedOnEveryRequest() async throws {
        router.register { _ in
            let body = #"{"providers":[]}"#.data(using: .utf8)!
            return .init(status: 200, body: body)
        }
        let client = HTTPClient(
            baseURL: URL(string: "http://127.0.0.1:8080")!,
            token: "tok-xyz",
            session: session
        )
        let providers = ProvidersGatewayAdapter(client: client)
        _ = try await providers.list()
        _ = try await providers.list()
        _ = try await providers.list()

        XCTAssertEqual(router.observed.count, 3)
        for r in router.observed {
            XCTAssertEqual(r.value(forHTTPHeaderField: "X-Harness-Token"), "tok-xyz")
        }
    }

    // 2. POST /v1/conversations round-trips CreateConversationRequest → Conversation.
    func test_createConversation_roundTrip_includingSandboxTemplateId() async throws {
        // Capture the request body payload that the client encoded.
        nonisolated(unsafe) var capturedBody: Data?
        router.register { request in
            // URLProtocol delivers the body via httpBodyStream; convert.
            if let stream = request.httpBodyStream {
                stream.open()
                defer { stream.close() }
                var data = Data()
                let bufSize = 4096
                let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: bufSize)
                defer { buffer.deallocate() }
                while stream.hasBytesAvailable {
                    let read = stream.read(buffer, maxLength: bufSize)
                    if read <= 0 { break }
                    data.append(buffer, count: read)
                }
                capturedBody = data
            } else if let body = request.httpBody {
                capturedBody = body
            }
            let resp = """
            {
              "id": "conv_1",
              "title": "Refactor harness-core",
              "provider_id": "claude",
              "model": "claude-sonnet-4-6",
              "sandbox_template_id": "tpl_strict_readonly",
              "created_at": "2026-04-28T10:15:00Z",
              "updated_at": "2026-04-28T10:15:00Z"
            }
            """.data(using: .utf8)!
            return .init(status: 201, body: resp)
        }

        let client = HTTPClient(
            baseURL: URL(string: "http://127.0.0.1:8080")!,
            token: "tok",
            session: session
        )
        let gateway = ConversationGatewayAdapter(client: client)

        let request = CreateConversationRequest(
            providerId: "claude",
            model: "claude-sonnet-4-6",
            title: "Refactor harness-core",
            sandboxTemplateId: "tpl_strict_readonly"
        )
        let conv = try await gateway.create(request)
        XCTAssertEqual(conv.id, "conv_1")
        XCTAssertEqual(conv.providerId, "claude")
        XCTAssertEqual(conv.model, "claude-sonnet-4-6")
        XCTAssertEqual(conv.sandboxTemplateId, "tpl_strict_readonly")

        XCTAssertNotNil(capturedBody)
        let decoded = try JSONDecoder().decode(CreateConversationRequest.self, from: capturedBody!)
        XCTAssertEqual(decoded, request)

        XCTAssertEqual(router.observed.first?.httpMethod, "POST")
        XCTAssertEqual(router.observed.first?.url?.path, "/v1/conversations")
        XCTAssertEqual(router.observed.first?.value(forHTTPHeaderField: "Content-Type"), "application/json")
    }
}
