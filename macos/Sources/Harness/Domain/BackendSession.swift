import Foundation

public struct BackendSession: Sendable, Equatable {
    public let baseURL: URL
    public let token: String

    public init(baseURL: URL, token: String) {
        self.baseURL = baseURL
        self.token = token
    }
}

public protocol BackendSessionProvider: Sendable {
    func acquire() async throws -> BackendSession
}

public enum BackendSessionError: Error, Equatable, Sendable {
    case backendNotFound(path: String)
    case spawnFailed(reason: String)
    case handshakeMalformed(line: String)
    case handshakeTimeout
    case backendExitedBeforeHandshake(status: Int32?)
}
