import XCTest
@testable import HarnessApp

final class HandshakeParserTests: XCTestCase {
    func testValidHandshakeYieldsBackendSession() throws {
        let session = try HandshakeParser.parse(#"{"port":8081,"token":"abc"}"#)
        XCTAssertEqual(session.baseURL, URL(string: "http://127.0.0.1:8081"))
        XCTAssertEqual(session.token, "abc")
    }

    func testTrailingNewlineAndWhitespaceTolerated() throws {
        let session = try HandshakeParser.parse("  {\"port\":9000,\"token\":\"xyz\"}\n")
        XCTAssertEqual(session.baseURL, URL(string: "http://127.0.0.1:9000"))
        XCTAssertEqual(session.token, "xyz")
    }

    func testMalformedJSONThrowsTypedError() {
        XCTAssertThrowsError(try HandshakeParser.parse("not json")) { err in
            guard case BackendSessionError.handshakeMalformed = err else {
                return XCTFail("expected handshakeMalformed, got \(err)")
            }
        }
    }

    func testMissingTokenThrowsTypedError() {
        XCTAssertThrowsError(try HandshakeParser.parse(#"{"port":8081}"#)) { err in
            guard case BackendSessionError.handshakeMalformed = err else {
                return XCTFail("expected handshakeMalformed, got \(err)")
            }
        }
    }

    func testInvalidPortThrowsTypedError() {
        XCTAssertThrowsError(try HandshakeParser.parse(#"{"port":0,"token":"x"}"#)) { err in
            guard case BackendSessionError.handshakeMalformed = err else {
                return XCTFail("expected handshakeMalformed, got \(err)")
            }
        }
    }

    func testEmptyTokenThrowsTypedError() {
        XCTAssertThrowsError(try HandshakeParser.parse(#"{"port":8081,"token":""}"#)) { err in
            guard case BackendSessionError.handshakeMalformed = err else {
                return XCTFail("expected handshakeMalformed, got \(err)")
            }
        }
    }
}

final class SidecarLauncherTests: XCTestCase {
    private func makeStubScript(_ body: String) throws -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("harness-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let scriptURL = dir.appendingPathComponent("stub.sh")
        let script = "#!/bin/sh\n\(body)\n"
        try script.write(to: scriptURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: scriptURL.path)
        return scriptURL
    }

    func testAcquireReadsHandshakeFromStdout() async throws {
        let stub = try makeStubScript(#"echo '{"port":8123,"token":"tok-abc"}'; sleep 30"#)
        let launcher = SidecarLauncher(executableURL: stub, handshakeTimeout: 3.0)
        defer { launcher.terminate() }

        let session = try await launcher.acquire()
        XCTAssertEqual(session.baseURL, URL(string: "http://127.0.0.1:8123"))
        XCTAssertEqual(session.token, "tok-abc")
    }

    func testMalformedHandshakeSurfacesTypedError() async throws {
        let stub = try makeStubScript("echo 'not json'; sleep 30")
        let launcher = SidecarLauncher(executableURL: stub, handshakeTimeout: 3.0)
        defer { launcher.terminate() }

        do {
            _ = try await launcher.acquire()
            XCTFail("expected throw")
        } catch BackendSessionError.handshakeMalformed {
            // expected
        } catch {
            XCTFail("expected handshakeMalformed, got \(error)")
        }
    }

    func testBackendExitsBeforeHandshakeYieldsTypedError() async throws {
        let stub = try makeStubScript("exit 7")
        let launcher = SidecarLauncher(executableURL: stub, handshakeTimeout: 3.0)
        defer { launcher.terminate() }

        do {
            _ = try await launcher.acquire()
            XCTFail("expected throw")
        } catch BackendSessionError.backendExitedBeforeHandshake {
            // expected
        } catch {
            XCTFail("expected backendExitedBeforeHandshake, got \(error)")
        }
    }

    func testMissingExecutableYieldsTypedError() async {
        let bogus = URL(fileURLWithPath: "/tmp/nonexistent-harness-\(UUID().uuidString)")
        let launcher = SidecarLauncher(executableURL: bogus, handshakeTimeout: 1.0)
        do {
            _ = try await launcher.acquire()
            XCTFail("expected throw")
        } catch BackendSessionError.backendNotFound {
            // expected
        } catch {
            XCTFail("expected backendNotFound, got \(error)")
        }
    }

    func testDeinitSendsSIGTERMToChild() throws {
        let pid: Int32
        do {
            let launcher = SidecarLauncher(
                executableURL: URL(fileURLWithPath: "/bin/sh"),
                arguments: ["-c", "sleep 30"]
            )
            pid = try launcher.spawnForTesting()
            XCTAssertTrue(processIsAlive(pid: pid), "child should be alive after spawn")
            // launcher goes out of scope here → deinit must terminate the child
        }

        // Poll for up to 5s for the child to disappear.
        let deadline = Date().addingTimeInterval(5.0)
        while Date() < deadline {
            if !processIsAlive(pid: pid) { return }
            Thread.sleep(forTimeInterval: 0.05)
        }
        XCTFail("child pid \(pid) still alive 5s after launcher deinit")
    }

    private func processIsAlive(pid: Int32) -> Bool {
        // kill(pid, 0) returns 0 if the process exists and we can signal it.
        // Returns -1 with errno=ESRCH if the process does not exist.
        // After Process.terminate() the child becomes a zombie until the
        // termination handler reaps it; we treat ESRCH as "gone".
        if kill(pid, 0) == 0 {
            return true
        }
        return errno != ESRCH
    }
}
