import Foundation

/// Tiny async semaphore (count = 0 default) for tests that need to gate
/// a fake gateway response so they can observe in-flight loading state.
/// Each `signal()` releases one waiter; `wait()` suspends until a signal
/// is available. Pre-existing signals queue (i.e. signal-then-wait
/// returns immediately).
final actor AsyncSemaphore {
    private var permits: Int
    private var waiters: [CheckedContinuation<Void, Never>] = []

    init(initialPermits: Int = 0) {
        self.permits = initialPermits
    }

    func signal() {
        if let next = waiters.first {
            waiters.removeFirst()
            next.resume()
        } else {
            permits += 1
        }
    }

    func wait() async {
        if permits > 0 {
            permits -= 1
            return
        }
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            waiters.append(continuation)
        }
    }
}
