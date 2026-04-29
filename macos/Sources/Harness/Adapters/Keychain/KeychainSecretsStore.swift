import Foundation
import Security

/// Thin wrapper around `SecItem*` so behavior tests can substitute a fake
/// implementation without touching the real macOS keychain. Production
/// callers use the default `SystemKeychainBackend`.
protocol KeychainBackend: Sendable {
    func copyMatching(_ query: [String: Any]) -> (status: OSStatus, data: Data?)
    func add(_ attributes: [String: Any]) -> OSStatus
}

struct SystemKeychainBackend: KeychainBackend {
    func copyMatching(_ query: [String: Any]) -> (status: OSStatus, data: Data?) {
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        return (status, item as? Data)
    }

    func add(_ attributes: [String: Any]) -> OSStatus {
        SecItemAdd(attributes as CFDictionary, nil)
    }
}

public final class KeychainSecretsStore: SecretsStore, @unchecked Sendable {
    static let keyByteCount = 32
    private static let account = "default"

    private let backend: KeychainBackend
    private let randomBytes: @Sendable (Int) -> Data?
    private let lock = NSLock()

    public convenience init() {
        self.init(backend: SystemKeychainBackend(), randomBytes: { count in
            KeychainSecretsStore.secRandom(count)
        })
    }

    init(backend: KeychainBackend, randomBytes: @escaping @Sendable (Int) -> Data?) {
        self.backend = backend
        self.randomBytes = randomBytes
    }

    public func storeOrFetchKey(forService service: String) async throws -> Data {
        // Serialise per-instance so two callers in the same process don't
        // race two `SecItemAdd`s. Cross-process races are handled below by
        // the duplicate-fallback (re-read on `errSecDuplicateItem`).
        lock.lock()
        defer { lock.unlock() }

        if let existing = try fetch(service: service) {
            return existing
        }
        guard let bytes = randomBytes(Self.keyByteCount) else {
            throw KeychainError.randomGenerationFailed(status: errSecAllocate)
        }
        if bytes.count != Self.keyByteCount {
            throw KeychainError.unexpectedKeyLength(bytes.count)
        }
        let attributes: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: Self.account,
            kSecValueData as String: bytes,
        ]
        let addStatus = backend.add(attributes)
        switch addStatus {
        case errSecSuccess:
            return bytes
        case errSecDuplicateItem:
            // Another process beat us to it; read theirs.
            if let existing = try fetch(service: service) {
                return existing
            }
            throw KeychainError.duplicate
        default:
            throw KeychainError.unhandled(status: addStatus)
        }
    }

    private func fetch(service: String) throws -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: Self.account,
            kSecMatchLimit as String: kSecMatchLimitOne,
            kSecReturnData as String: true,
        ]
        let result = backend.copyMatching(query)
        switch result.status {
        case errSecSuccess:
            guard let data = result.data else { return nil }
            if data.count != Self.keyByteCount {
                throw KeychainError.unexpectedKeyLength(data.count)
            }
            return data
        case errSecItemNotFound:
            return nil
        default:
            throw KeychainError.unhandled(status: result.status)
        }
    }

    private static func secRandom(_ count: Int) -> Data? {
        var bytes = Data(count: count)
        let status = bytes.withUnsafeMutableBytes { ptr -> Int32 in
            guard let base = ptr.baseAddress else { return errSecAllocate }
            return SecRandomCopyBytes(kSecRandomDefault, count, base)
        }
        return status == errSecSuccess ? bytes : nil
    }
}

extension Data {
    /// Lowercase hex encoding for `HARNESS_DB_KEY_HEX`.
    func hexEncodedString() -> String {
        var s = String()
        s.reserveCapacity(count * 2)
        for b in self {
            s.append(String(format: "%02x", b))
        }
        return s
    }
}
