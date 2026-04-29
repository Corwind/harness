import XCTest
@testable import HarnessApp

final class SidecarLauncherEnvInjectionTests: XCTestCase {
    private func makeEnvCaptureStub(envFile: URL) throws -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("harness-env-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let scriptURL = dir.appendingPathComponent("stub.sh")
        // Capture the env to a file *before* emitting the handshake so the
        // file is written by the time `acquire()` returns.
        let body = """
        #!/bin/sh
        env > '\(envFile.path)'
        echo '{"port":8200,"token":"tok-env"}'
        sleep 30
        """
        try body.write(to: scriptURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o755], ofItemAtPath: scriptURL.path)
        return scriptURL
    }

    private func readEnvFile(_ url: URL) throws -> [String: String] {
        let raw = try String(contentsOf: url, encoding: .utf8)
        var out: [String: String] = [:]
        for line in raw.split(separator: "\n", omittingEmptySubsequences: true) {
            if let eq = line.firstIndex(of: "=") {
                let k = String(line[..<eq])
                let v = String(line[line.index(after: eq)...])
                out[k] = v
            }
        }
        return out
    }

    func testInjectsHexKeyAndDbPathFromSecretsStore() async throws {
        let envCapture = FileManager.default.temporaryDirectory
            .appendingPathComponent("harness-env-\(UUID().uuidString).txt")
        let stub = try makeEnvCaptureStub(envFile: envCapture)

        let fakeKey = Data(repeating: 0xCD, count: 32)
        let store = StaticSecretsStore(key: fakeKey)
        let dbDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("harness-db-\(UUID().uuidString)", isDirectory: true)
        let dbPath = dbDir.appendingPathComponent("harness.sqlite")

        let launcher = SidecarLauncher(
            executableURL: stub,
            environment: nil,
            secretsStore: store,
            dbPathOverride: dbPath,
            handshakeTimeout: 3.0
        )
        defer { launcher.terminate() }

        _ = try await launcher.acquire()

        // Give the stub a moment to flush the env file (it writes before echo,
        // so by the time handshake arrives the file should exist; but be
        // defensive against fs race).
        var captured: [String: String] = [:]
        for _ in 0..<20 {
            if FileManager.default.fileExists(atPath: envCapture.path) {
                captured = try readEnvFile(envCapture)
                if captured["HARNESS_DB_KEY_HEX"] != nil { break }
            }
            try await Task.sleep(nanoseconds: 50_000_000)
        }

        XCTAssertEqual(captured["HARNESS_DB_KEY_HEX"], String(repeating: "cd", count: 32))
        XCTAssertEqual(captured["HARNESS_DB_PATH"], dbPath.path)
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: dbDir.path),
            "parent dir for HARNESS_DB_PATH should be created")
    }

    func testCallerProvidedKeyHexOverridesSecretsStore() async throws {
        let envCapture = FileManager.default.temporaryDirectory
            .appendingPathComponent("harness-env-\(UUID().uuidString).txt")
        let stub = try makeEnvCaptureStub(envFile: envCapture)

        let storeKey = Data(repeating: 0x11, count: 32)
        let store = StaticSecretsStore(key: storeKey)

        let devOverride = String(repeating: "0", count: 64)
        let env = ["HARNESS_DB_KEY_HEX": devOverride]

        let launcher = SidecarLauncher(
            executableURL: stub,
            environment: env,
            secretsStore: store,
            handshakeTimeout: 3.0
        )
        defer { launcher.terminate() }

        _ = try await launcher.acquire()

        var captured: [String: String] = [:]
        for _ in 0..<20 {
            if FileManager.default.fileExists(atPath: envCapture.path) {
                captured = try readEnvFile(envCapture)
                if captured["HARNESS_DB_KEY_HEX"] != nil { break }
            }
            try await Task.sleep(nanoseconds: 50_000_000)
        }

        XCTAssertEqual(captured["HARNESS_DB_KEY_HEX"], devOverride,
                       "caller-supplied HARNESS_DB_KEY_HEX must take precedence over SecretsStore")
    }

    func testCallerProvidedDbPathOverridesDefault() async throws {
        let envCapture = FileManager.default.temporaryDirectory
            .appendingPathComponent("harness-env-\(UUID().uuidString).txt")
        let stub = try makeEnvCaptureStub(envFile: envCapture)

        let store = StaticSecretsStore(key: Data(repeating: 0x22, count: 32))
        let callerPath = "/tmp/harness-test-caller-\(UUID().uuidString).sqlite"
        let env = ["HARNESS_DB_PATH": callerPath]

        let launcher = SidecarLauncher(
            executableURL: stub,
            environment: env,
            secretsStore: store,
            handshakeTimeout: 3.0
        )
        defer { launcher.terminate() }

        _ = try await launcher.acquire()

        var captured: [String: String] = [:]
        for _ in 0..<20 {
            if FileManager.default.fileExists(atPath: envCapture.path) {
                captured = try readEnvFile(envCapture)
                if captured["HARNESS_DB_PATH"] != nil { break }
            }
            try await Task.sleep(nanoseconds: 50_000_000)
        }

        XCTAssertEqual(captured["HARNESS_DB_PATH"], callerPath)
    }

    func testNoSecretsStoreAndNoOverrideLeavesEnvUnchanged() async throws {
        let envCapture = FileManager.default.temporaryDirectory
            .appendingPathComponent("harness-env-\(UUID().uuidString).txt")
        let stub = try makeEnvCaptureStub(envFile: envCapture)

        let launcher = SidecarLauncher(
            executableURL: stub,
            environment: ["FOO": "bar"],
            secretsStore: nil,
            handshakeTimeout: 3.0
        )
        defer { launcher.terminate() }

        _ = try await launcher.acquire()

        var captured: [String: String] = [:]
        for _ in 0..<20 {
            if FileManager.default.fileExists(atPath: envCapture.path) {
                captured = try readEnvFile(envCapture)
                if captured["FOO"] != nil { break }
            }
            try await Task.sleep(nanoseconds: 50_000_000)
        }

        XCTAssertEqual(captured["FOO"], "bar")
        XCTAssertNil(captured["HARNESS_DB_KEY_HEX"], "no SecretsStore → no key injection")
    }
}

private final class StaticSecretsStore: SecretsStore, @unchecked Sendable {
    let key: Data
    init(key: Data) { self.key = key }
    func storeOrFetchKey(forService service: String) async throws -> Data { key }
}
