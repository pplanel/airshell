import Dispatch
import Network
import Synchronization
import WiFiAware

/// Publishes a service to all paired devices and queues incoming connections
/// for blocking `accept` calls from Rust.
final class Listener: Sendable {
    private struct Queue {
        var connections: [Connection] = []
        var closed = false
    }

    private let queue = Mutex(Queue())
    private let available = DispatchSemaphore(value: 0)
    private let task = Mutex<Task<Void, Never>?>(nil)

    init(publishing service: WAPublishableService) {
        let task = Task {
            do {
                try await NetworkListener(
                    for: .wifiAware(.connecting(to: service, from: .allPairedDevices)),
                    using: .parameters { TCP().noDelay(true) }
                )
                .run { networkConnection in
                    // The Connection keeps the NetworkConnection alive after this returns.
                    self.enqueue(Connection(networkConnection))
                }
            } catch {
                logger.error("listener: \(error)")
            }
            self.close()
        }
        self.task.withLock { $0 = task }
    }

    /// Block until a paired device connects; nil once the listener is closed.
    func accept() -> Connection? {
        available.wait()
        let next: Connection? = queue.withLock { queue in
            queue.connections.isEmpty ? nil : queue.connections.removeFirst()
        }
        if next == nil {
            available.signal() // Closed: pass the wakeup on to any other waiter.
        }
        return next
    }

    /// Stop publishing, cancel unaccepted connections, and unblock `accept`.
    /// Connections already accepted stay up. Idempotent.
    func close() {
        let unaccepted: [Connection]? = queue.withLock { queue in
            guard !queue.closed else { return nil }
            queue.closed = true
            defer { queue.connections.removeAll() }
            return queue.connections
        }
        guard let unaccepted else { return }
        unaccepted.forEach { $0.cancel() }
        available.signal()
        task.withLock { $0?.cancel() }
    }

    private func enqueue(_ connection: Connection) {
        let queued = queue.withLock { queue in
            guard !queue.closed else { return false }
            queue.connections.append(connection)
            return true
        }
        if queued {
            available.signal()
        } else {
            connection.cancel()
        }
    }
}
