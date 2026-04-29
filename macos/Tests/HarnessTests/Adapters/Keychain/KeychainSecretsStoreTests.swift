import XCTest
import Security
@testable import HarnessApp

final class KeychainSecretsStoreTests: XCTestCase {
    private var serviceIdsToCleanUp: [String] = []

    override func tearDown() {
        // Best-effort wipe so tests don't pollute the developer keychain.
        for service in serviceIdsToCleanUp {
            let query: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: service,
            ]
            SecItemDelete(query as CFDictionary)
        }
        serviceIdsToCleanUp.removeAll()
        super.tearDown()
    }

    private func uniqueService(file: StaticString = #file, line: UInt = #line) -> String {
        let s = "com.harness.test.\(UUID().uuidString)"
        serviceIdsToCleanUp.append(s)
        return s
    }

    func testFirstRunGeneratesAndStores32ByteKey() async throws {
        try skipIfKeychainUnavailable()
        let store = KeychainSecretsStore()
        let service = uniqueService()

        let first = try await store.storeOrFetchKey(forService: service)
        XCTAssertEqual(first.count, 32)

        let second = try await store.storeOrFetchKey(forService: service)
        XCTAssertEqual(second, first, "second fetch should return the same bytes")
    }

    func testTwoIndependentStoreInstancesShareTheSameKeyForOneService() async throws {
        try skipIfKeychainUnavailable()
        let service = uniqueService()

        let storeA = KeychainSecretsStore()
        let storeB = KeychainSecretsStore()
        let a = try await storeA.storeOrFetchKey(forService: service)
        let b = try await storeB.storeOrFetchKey(forService: service)

        XCTAssertEqual(a, b)
        XCTAssertEqual(a.count, 32)
    }

    func testParallelCallsConvergeOnSameKey() async throws {
        try skipIfKeychainUnavailable()
        let store = KeychainSecretsStore()
        let service = uniqueService()

        async let r1 = store.storeOrFetchKey(forService: service)
        async let r2 = store.storeOrFetchKey(forService: service)
        async let r3 = store.storeOrFetchKey(forService: service)
        let (a, b, c) = try await (r1, r2, r3)

        XCTAssertEqual(a, b)
        XCTAssertEqual(b, c)
        XCTAssertEqual(a.count, 32)
    }

    /// macOS CI runs without an unlocked keychain may return errSecMissingEntitlement
    /// (-34018) or similar. Skip rather than fail in that environment.
    private func skipIfKeychainUnavailable() throws {
        let probeService = "com.harness.test.probe.\(UUID().uuidString)"
        defer {
            let q: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: probeService,
            ]
            SecItemDelete(q as CFDictionary)
        }
        let attrs: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: probeService,
            kSecAttrAccount as String: "probe",
            kSecValueData as String: Data([0x00]),
        ]
        let status = SecItemAdd(attrs as CFDictionary, nil)
        if status != errSecSuccess {
            throw XCTSkip("Keychain unavailable in this environment (OSStatus \(status))")
        }
    }
}

final class KeychainSecretsStoreFakeBackendTests: XCTestCase {
    func testFakeBackendRoundTripsKey() async throws {
        let backend = FakeKeychainBackend()
        let store = KeychainSecretsStore(
            backend: backend,
            randomBytes: { count in Data(repeating: 0xAB, count: count) }
        )
        let service = "fake-service"

        let first = try await store.storeOrFetchKey(forService: service)
        XCTAssertEqual(first.count, 32)
        XCTAssertTrue(first.allSatisfy { $0 == 0xAB })

        let second = try await store.storeOrFetchKey(forService: service)
        XCTAssertEqual(second, first)
        XCTAssertEqual(backend.addCallCount, 1, "second call must not re-add")
    }

    func testDuplicateOnAddFallsBackToFetch() async throws {
        let backend = FakeKeychainBackend()
        // Pre-populate with a key so the next `add` returns errSecDuplicateItem.
        let preexisting = Data(repeating: 0x42, count: 32)
        backend.preset(service: "svc", data: preexisting)
        // Force fake to return notFound on first lookup, then return preexisting
        // on the post-duplicate retry.
        backend.scriptedCopyResults = [
            (errSecItemNotFound, nil),
            (errSecSuccess, preexisting),
        ]
        backend.scriptedAddStatuses = [errSecDuplicateItem]

        let store = KeychainSecretsStore(
            backend: backend,
            randomBytes: { Data(repeating: 0x99, count: $0) }
        )
        let result = try await store.storeOrFetchKey(forService: "svc")

        XCTAssertEqual(result, preexisting,
                       "duplicate-on-add must surface the keychain's existing value, not our random bytes")
    }

    func testUnhandledOSStatusOnAddSurfacesAsTypedError() async {
        let backend = FakeKeychainBackend()
        backend.scriptedCopyResults = [(errSecItemNotFound, nil)]
        backend.scriptedAddStatuses = [-25300] // errSecItemNotFound on the wrong call path
        let store = KeychainSecretsStore(
            backend: backend,
            randomBytes: { Data(repeating: 0x01, count: $0) }
        )
        do {
            _ = try await store.storeOrFetchKey(forService: "svc")
            XCTFail("expected throw")
        } catch KeychainError.unhandled(let status) {
            XCTAssertEqual(status, -25300)
        } catch {
            XCTFail("expected KeychainError.unhandled, got \(error)")
        }
    }

    func testUnhandledOSStatusOnFetchSurfacesAsTypedError() async {
        let backend = FakeKeychainBackend()
        backend.scriptedCopyResults = [(-25295, nil)] // errSecAuthFailed-ish
        let store = KeychainSecretsStore(
            backend: backend,
            randomBytes: { Data(repeating: 0x01, count: $0) }
        )
        do {
            _ = try await store.storeOrFetchKey(forService: "svc")
            XCTFail("expected throw")
        } catch KeychainError.unhandled(let status) {
            XCTAssertEqual(status, -25295)
        } catch {
            XCTFail("expected KeychainError.unhandled, got \(error)")
        }
    }

    func testUnexpectedKeyLengthOnFetchSurfacesAsTypedError() async {
        let backend = FakeKeychainBackend()
        backend.scriptedCopyResults = [(errSecSuccess, Data(count: 16))] // wrong size
        let store = KeychainSecretsStore(
            backend: backend,
            randomBytes: { Data(repeating: 0x01, count: $0) }
        )
        do {
            _ = try await store.storeOrFetchKey(forService: "svc")
            XCTFail("expected throw")
        } catch KeychainError.unexpectedKeyLength(let n) {
            XCTAssertEqual(n, 16)
        } catch {
            XCTFail("expected KeychainError.unexpectedKeyLength, got \(error)")
        }
    }

    func testRandomGenerationFailureSurfacesAsTypedError() async {
        let backend = FakeKeychainBackend()
        backend.scriptedCopyResults = [(errSecItemNotFound, nil)]
        let store = KeychainSecretsStore(
            backend: backend,
            randomBytes: { _ in nil }
        )
        do {
            _ = try await store.storeOrFetchKey(forService: "svc")
            XCTFail("expected throw")
        } catch KeychainError.randomGenerationFailed {
            // expected
        } catch {
            XCTFail("expected KeychainError.randomGenerationFailed, got \(error)")
        }
    }
}

final class HexEncodingTests: XCTestCase {
    func testKnownVectorsRoundTrip() {
        XCTAssertEqual(Data([0x00, 0xFF, 0xAB, 0x10]).hexEncodedString(), "00ffab10")
        XCTAssertEqual(Data(repeating: 0xAA, count: 4).hexEncodedString(), "aaaaaaaa")
        XCTAssertEqual(Data().hexEncodedString(), "")
        // 32-byte key produces 64-char hex.
        XCTAssertEqual(Data(repeating: 0x42, count: 32).hexEncodedString().count, 64)
    }
}

final class FakeKeychainBackend: KeychainBackend, @unchecked Sendable {
    private let lock = NSLock()
    private var storage: [String: Data] = [:] // service id -> data
    private(set) var addCallCount = 0

    /// If non-empty, drains scripted results in order; otherwise falls back to
    /// real in-memory behavior.
    var scriptedCopyResults: [(OSStatus, Data?)] = []
    var scriptedAddStatuses: [OSStatus] = []

    func preset(service: String, data: Data) {
        lock.lock(); defer { lock.unlock() }
        storage[service] = data
    }

    func copyMatching(_ query: [String: Any]) -> (status: OSStatus, data: Data?) {
        lock.lock(); defer { lock.unlock() }
        if !scriptedCopyResults.isEmpty {
            return scriptedCopyResults.removeFirst()
        }
        guard let service = query[kSecAttrService as String] as? String else {
            return (errSecParam, nil)
        }
        if let d = storage[service] {
            return (errSecSuccess, d)
        }
        return (errSecItemNotFound, nil)
    }

    func add(_ attributes: [String: Any]) -> OSStatus {
        lock.lock(); defer { lock.unlock() }
        addCallCount += 1
        if !scriptedAddStatuses.isEmpty {
            return scriptedAddStatuses.removeFirst()
        }
        guard let service = attributes[kSecAttrService as String] as? String,
              let data = attributes[kSecValueData as String] as? Data else {
            return errSecParam
        }
        if storage[service] != nil { return errSecDuplicateItem }
        storage[service] = data
        return errSecSuccess
    }
}
