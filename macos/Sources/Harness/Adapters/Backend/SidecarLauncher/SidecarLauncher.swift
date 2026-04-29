import Foundation

enum HandshakeParser {
    static func parse(_ line: String) throws -> BackendSession {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let data = trimmed.data(using: .utf8) else {
            throw BackendSessionError.handshakeMalformed(line: line)
        }
        guard
            let object = try? JSONSerialization.jsonObject(with: data, options: []),
            let dict = object as? [String: Any],
            let port = dict["port"] as? Int,
            let token = dict["token"] as? String,
            (1...65535).contains(port),
            !token.isEmpty,
            let url = URL(string: "http://127.0.0.1:\(port)")
        else {
            throw BackendSessionError.handshakeMalformed(line: line)
        }
        return BackendSession(baseURL: url, token: token)
    }
}

public final class SidecarLauncher: BackendSessionProvider, @unchecked Sendable {
    public static let keyServiceIdDefault = "com.harness.encryption-key"
    static let envKeyHex = "HARNESS_DB_KEY_HEX"
    static let envDbPath = "HARNESS_DB_PATH"

    private let executableURL: URL
    private let arguments: [String]
    private let environment: [String: String]?
    private let secretsStore: SecretsStore?
    private let dbPathOverride: URL?
    private let keyServiceId: String
    private let handshakeTimeout: TimeInterval

    private let lock = NSLock()
    private var process: Process?
    private var session: BackendSession?

    public init(
        executableURL: URL,
        arguments: [String] = [],
        environment: [String: String]? = nil,
        secretsStore: SecretsStore? = nil,
        dbPathOverride: URL? = nil,
        keyServiceId: String = SidecarLauncher.keyServiceIdDefault,
        handshakeTimeout: TimeInterval = 5.0
    ) {
        self.executableURL = executableURL
        self.arguments = arguments
        self.environment = environment
        self.secretsStore = secretsStore
        self.dbPathOverride = dbPathOverride
        self.keyServiceId = keyServiceId
        self.handshakeTimeout = handshakeTimeout
    }

    deinit {
        terminate()
    }

    public func acquire() async throws -> BackendSession {
        lock.lock()
        if let cached = session {
            lock.unlock()
            return cached
        }
        lock.unlock()

        guard FileManager.default.isExecutableFile(atPath: executableURL.path) else {
            throw BackendSessionError.backendNotFound(path: executableURL.path)
        }

        let resolvedEnv = try await resolveEnvironment()

        let proc = Process()
        proc.executableURL = executableURL
        proc.arguments = arguments
        if let resolvedEnv {
            proc.environment = resolvedEnv
        }

        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        proc.standardOutput = stdoutPipe
        proc.standardError = stderrPipe

        do {
            try proc.run()
        } catch {
            throw BackendSessionError.spawnFailed(reason: String(describing: error))
        }

        lock.lock()
        process = proc
        lock.unlock()

        let handle = stdoutPipe.fileHandleForReading
        let timeout = handshakeTimeout

        let line: String
        do {
            line = try await Self.readFirstLine(from: handle, process: proc, timeout: timeout)
        } catch {
            terminate()
            throw error
        }

        let parsed: BackendSession
        do {
            parsed = try HandshakeParser.parse(line)
        } catch {
            terminate()
            throw error
        }

        lock.lock()
        session = parsed
        lock.unlock()

        return parsed
    }

    /// Builds the env passed to the spawned backend. Caller-supplied values
    /// always win; the SecretsStore (if any) only fills missing slots — this
    /// is what lets dev/test callers override `HARNESS_DB_KEY_HEX` and
    /// `HARNESS_DB_PATH` without ever touching the user's keychain.
    private func resolveEnvironment() async throws -> [String: String]? {
        // If the caller hasn't supplied an environment AND we have nothing to
        // inject, leave it nil so the child inherits the parent env (existing
        // behavior).
        if environment == nil && secretsStore == nil && dbPathOverride == nil {
            return nil
        }
        var env = environment ?? [:]

        // Only consult the secrets store when the caller hasn't pre-set the
        // key. Pairs naturally with the dev override path: callers that set
        // HARNESS_DB_KEY_HEX skip the keychain entirely.
        let consultedSecretsStore: Bool
        if env[Self.envKeyHex] == nil, let secretsStore {
            let bytes = try await secretsStore.storeOrFetchKey(forService: keyServiceId)
            env[Self.envKeyHex] = bytes.hexEncodedString()
            consultedSecretsStore = true
        } else {
            consultedSecretsStore = false
        }

        if env[Self.envDbPath] == nil {
            if let dbPathOverride {
                try Self.ensureParentDirectory(of: dbPathOverride)
                env[Self.envDbPath] = dbPathOverride.path
            } else if consultedSecretsStore {
                // We're on the production path (keychain-managed key, no
                // explicit overrides). Default the DB to Application Support.
                let path = Self.defaultDatabasePath()
                try Self.ensureParentDirectory(of: path)
                env[Self.envDbPath] = path.path
            }
        }

        return env
    }

    static func defaultDatabasePath() -> URL {
        let fm = FileManager.default
        let base: URL
        if let appSupport = try? fm.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: false
        ) {
            base = appSupport.appendingPathComponent("Harness", isDirectory: true)
        } else {
            base = URL(fileURLWithPath: NSHomeDirectory())
                .appendingPathComponent("Library/Application Support/Harness", isDirectory: true)
        }
        return base.appendingPathComponent("harness.sqlite")
    }

    private static func ensureParentDirectory(of file: URL) throws {
        let parent = file.deletingLastPathComponent()
        try FileManager.default.createDirectory(
            at: parent, withIntermediateDirectories: true)
    }

    /// Internal hook for behavior tests: spawns the child without awaiting the handshake.
    /// Returns the child PID. Caller can then drop the launcher to verify SIGTERM-on-deinit.
    func spawnForTesting() throws -> Int32 {
        let proc = Process()
        proc.executableURL = executableURL
        proc.arguments = arguments
        if let environment {
            proc.environment = environment
        }
        proc.standardOutput = Pipe()
        proc.standardError = Pipe()
        do {
            try proc.run()
        } catch {
            throw BackendSessionError.spawnFailed(reason: String(describing: error))
        }
        lock.lock()
        process = proc
        lock.unlock()
        return proc.processIdentifier
    }

    public func terminate() {
        lock.lock()
        let proc = process
        process = nil
        session = nil
        lock.unlock()

        guard let proc, proc.isRunning else { return }
        proc.terminate()

        let deadline = Date().addingTimeInterval(5.0)
        while proc.isRunning && Date() < deadline {
            Thread.sleep(forTimeInterval: 0.05)
        }
        if proc.isRunning {
            kill(proc.processIdentifier, SIGKILL)
        }
    }

    private static func readFirstLine(
        from handle: FileHandle,
        process: Process,
        timeout: TimeInterval
    ) async throws -> String {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<String, Error>) in
            let state = ReadState()

            let finish: (Result<String, Error>) -> Void = { result in
                guard state.complete() else { return }
                handle.readabilityHandler = nil
                process.terminationHandler = nil
                continuation.resume(with: result)
            }

            handle.readabilityHandler = { fh in
                let chunk = fh.availableData
                if chunk.isEmpty {
                    finish(.failure(BackendSessionError.backendExitedBeforeHandshake(
                        status: process.isRunning ? nil : process.terminationStatus
                    )))
                    return
                }
                state.buffer.append(chunk)
                if let newlineIdx = state.buffer.firstIndex(of: 0x0A) {
                    let lineData = state.buffer.prefix(newlineIdx)
                    let line = String(data: Data(lineData), encoding: .utf8) ?? ""
                    finish(.success(line))
                }
            }

            process.terminationHandler = { proc in
                finish(.failure(BackendSessionError.backendExitedBeforeHandshake(
                    status: proc.terminationStatus
                )))
            }

            let deadline = DispatchTime.now() + timeout
            DispatchQueue.global().asyncAfter(deadline: deadline) {
                finish(.failure(BackendSessionError.handshakeTimeout))
            }
        }
    }
}

private final class ReadState: @unchecked Sendable {
    var buffer = Data()
    private let lock = NSLock()
    private var done = false

    func complete() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        if done { return false }
        done = true
        return true
    }
}
