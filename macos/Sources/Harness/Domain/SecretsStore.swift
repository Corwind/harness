import Foundation

/// Persists a single 32-byte key per service id. First call generates and
/// stores; subsequent calls return the same bytes. Concurrency-safe per
/// service id (parallel callers receive identical bytes).
public protocol SecretsStore: Sendable {
    func storeOrFetchKey(forService service: String) async throws -> Data
}

public enum KeychainError: Error, Equatable, Sendable {
    case notFound
    case duplicate
    case unexpectedKeyLength(Int)
    case randomGenerationFailed(status: Int32)
    case unhandled(status: Int32)
}
