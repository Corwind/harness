import Foundation
@testable import HarnessApp

/// Test scaffolding that spawns the real `harness-server` binary and
/// surfaces the resulting `BackendSession` for end-to-end tests.
///
/// Skipping rule: if the binary cannot be located the harness throws
/// `LiveBackendHarnessError.binaryUnavailable` so the test caller can
/// `throw XCTSkip(...)` rather than fail. CI without the build artefact
/// still runs the rest of the suite.
final class LiveBackendHarness {
    enum Error: Swift.Error, CustomStringConvertible {
        case binaryUnavailable(searched: [String])
        case spawnFailed(reason: String)
        case handshakeFailed(reason: String)

        var description: String {
            switch self {
            case .binaryUnavailable(let paths):
                return "harness-server binary not found in \(paths)"
            case .spawnFailed(let r):
                return "spawn failed: \(r)"
            case .handshakeFailed(let r):
                return "handshake failed: \(r)"
            }
        }
    }

    let binaryURL: URL
    let session: BackendSession
    private let launcher: SidecarLauncher
    let dbPath: URL
    private let tempDir: URL

    static func locateBinary() -> URL? {
        if let env = ProcessInfo.processInfo.environment["HARNESS_BACKEND_PATH"],
           !env.isEmpty,
           FileManager.default.isExecutableFile(atPath: env) {
            return URL(fileURLWithPath: env)
        }
        let candidates = [
            // Workspace-relative paths from the package root.
            "../backend/target/debug/harness-server",
            "../backend/target/release/harness-server",
            // Repo-root absolute (developer machines).
            "/Users/guillaumedore/perso/harness/backend/target/debug/harness-server",
            "/Users/guillaumedore/perso/harness/backend/target/release/harness-server",
        ]
        for path in candidates {
            if FileManager.default.isExecutableFile(atPath: path) {
                return URL(fileURLWithPath: path).standardizedFileURL
            }
        }
        return nil
    }

    init(extraEnv: [String: String] = [:]) async throws {
        guard let binary = Self.locateBinary() else {
            throw Error.binaryUnavailable(searched: [
                "$HARNESS_BACKEND_PATH",
                "../backend/target/{debug,release}/harness-server",
            ])
        }
        self.binaryURL = binary

        let tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("harness-e2e-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(
            at: tempDir, withIntermediateDirectories: true
        )
        self.tempDir = tempDir
        self.dbPath = tempDir.appendingPathComponent("harness.sqlite")

        var env = ProcessInfo.processInfo.environment
        env["HARNESS_DB_PATH"] = self.dbPath.path
        // 32-byte all-zero key for tests; never used outside of tests.
        env["HARNESS_DB_KEY_HEX"] = String(repeating: "0", count: 64)
        env["HARNESS_FAKE_PROVIDER"] = "1"
        for (k, v) in extraEnv { env[k] = v }

        let launcher = SidecarLauncher(
            executableURL: binary,
            arguments: [],
            environment: env,
            handshakeTimeout: 10.0
        )
        self.launcher = launcher

        do {
            self.session = try await launcher.acquire()
        } catch {
            launcher.terminate()
            throw Error.handshakeFailed(reason: String(describing: error))
        }
    }

    func shutdown() {
        launcher.terminate()
        try? FileManager.default.removeItem(at: tempDir)
    }
}
