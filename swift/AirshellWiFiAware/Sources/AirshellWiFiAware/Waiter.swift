import Dispatch
import Synchronization

/// A one-shot result that a non-Swift thread (Rust) blocks on. The first
/// `resolve` wins; later ones are ignored.
///
/// Never wait from Swift concurrency: it would block a cooperative thread.
final class Waiter<T: Sendable>: Sendable {
    private let result = Mutex<Result<T, any Error>?>(nil)
    private let ready = DispatchSemaphore(value: 0)

    func resolve(_ value: Result<T, any Error>) {
        let first = result.withLock { result in
            guard result == nil else { return false }
            result = value
            return true
        }
        if first {
            ready.signal()
        }
    }

    func wait() -> Result<T, any Error> {
        ready.wait()
        return result.withLock { $0! }
    }

    /// Nil if nothing resolved within `timeout` seconds.
    func wait(timeout: Double) -> Result<T, any Error>? {
        guard ready.wait(timeout: .now() + max(timeout, 0)) == .success else { return nil }
        return result.withLock { $0! }
    }
}
