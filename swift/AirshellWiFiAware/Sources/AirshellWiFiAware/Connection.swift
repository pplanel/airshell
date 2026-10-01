import Foundation
import Network
import WiFiAware

private let browserQueue = DispatchQueue(label: "airshell.wifi-aware.browser")

/// A Wi-Fi Aware TCP connection driven by blocking calls from Rust.
///
/// A receiver task reads into a buffer from the start, which also starts the
/// connection. `receive` blocks on that buffer and `send` on a waiter, so
/// `cancel` wakes both directly instead of relying on how Network.framework
/// reacts to task cancellation.
final class Connection: @unchecked Sendable {
    private static let chunk = 64 * 1024

    private let connection: NetworkConnection<TCP>
    // Everything below is guarded by `condition`.
    private let condition = NSCondition()
    private var buffer = Data()
    private var ready = false
    private var peerFinished = false
    private var failed = false
    private var cancelled = false
    private var sends: [ObjectIdentifier: Waiter<Void>] = [:]
    private var receiver: Task<Void, Never>?

    init(_ connection: NetworkConnection<TCP>) {
        self.connection = connection
        connection.onStateUpdate { [weak self] _, state in
            self?.update(state)
        }
        let receiver = Task { [weak self, connection] in
            do {
                while true {
                    let message = try await connection.receive(atLeast: 1, atMost: Connection.chunk)
                    guard let self else { return }
                    self.deliver(message.content, endOfStream: message.metadata.endOfStream)
                    if message.metadata.endOfStream {
                        return
                    }
                }
            } catch {
                self?.fail(error)
            }
        }
        condition.withLock { self.receiver = receiver }
    }

    deinit {
        receiver?.cancel()
    }

    /// Find a paired device publishing `service` (the one named `deviceName`,
    /// or the first found) and connect to it. Nil on failure or timeout.
    static func connect(
        to service: WASubscribableService,
        deviceName: String?,
        timeout: TimeInterval
    ) -> Connection? {
        let deadline = Date(timeIntervalSinceNow: timeout)
        let subscriber: WASubscriberBrowser = .wifiAware(.connecting(to: .allPairedDevices, from: service))
        // NWBrowser rather than NetworkBrowser.run: finishing `run` stops the
        // subscription, and the data path needs it until the connection is ready.
        let browser = NWBrowser(for: subscriber.makeDescriptor(), using: subscriber.configureParameters(nil))
        defer { browser.cancel() }

        let found = Waiter<WAEndpoint>()
        browser.stateUpdateHandler = { state in
            if case .failed(let error) = state {
                found.resolve(.failure(error))
            }
        }
        browser.browseResultsChangedHandler = { results, _ in
            for result in results {
                if let endpoint = try? subscriber.makeEndpoint(from: result),
                    deviceName == nil || endpoint.device.name == deviceName
                {
                    found.resolve(.success(endpoint))
                    return
                }
            }
        }
        browser.start(queue: browserQueue)

        switch found.wait(timeout: deadline.timeIntervalSinceNow) {
        case .success(let endpoint):
            let connection = Connection(NetworkConnection(to: endpoint, using: .parameters { TCP().noDelay(true) }))
            guard connection.waitUntilReady(before: deadline) else {
                logger.error("connect: \(endpoint) did not become ready within the timeout")
                connection.cancel()
                return nil
            }
            return connection
        case .failure(let error):
            logger.error("connect: \(error)")
            return nil
        case nil:
            logger.error("connect: no paired device publishing \(service.name) within the timeout")
            return nil
        }
    }

    /// Up to `maxLength` bytes; empty once the peer has finished sending; nil
    /// if the connection failed or was cancelled.
    func receive(atMost maxLength: Int) -> Data? {
        condition.withLock {
            while buffer.isEmpty && !peerFinished && !failed && !cancelled {
                condition.wait()
            }
            if cancelled {
                return nil
            }
            guard !buffer.isEmpty else { return failed ? nil : Data() }
            let chunk = Data(buffer.prefix(maxLength))
            buffer.removeFirst(chunk.count)
            return chunk
        }
    }

    /// False if the send failed or the connection was cancelled meanwhile.
    func send(_ data: Data) -> Bool {
        let waiter = Waiter<Void>()
        let id = ObjectIdentifier(waiter)
        let started = condition.withLock {
            guard !cancelled && !failed else { return false }
            sends[id] = waiter
            return true
        }
        guard started else { return false }
        Task { [connection] in
            do {
                try await connection.send(data)
                waiter.resolve(.success(()))
            } catch {
                waiter.resolve(.failure(error))
            }
        }
        let result = waiter.wait()
        condition.withLock { _ = sends.removeValue(forKey: id) }
        if case .failure = result {
            return false
        }
        return true
    }

    /// Fail pending calls, send a FIN if the connection is up, then stop
    /// receiving. Idempotent.
    func cancel() {
        let pending: (sends: [Waiter<Void>], receiver: Task<Void, Never>?, ready: Bool)? = condition.withLock {
            guard !cancelled else { return nil }
            cancelled = true
            condition.broadcast()
            defer { sends.removeAll() }
            return (Array(sends.values), receiver, ready)
        }
        guard let pending else { return }
        for send in pending.sends {
            send.resolve(.failure(CancellationError()))
        }
        guard pending.ready else {
            pending.receiver?.cancel()
            return
        }
        Task { [connection] in
            try? await connection.send(Data(), endOfStream: true)
            pending.receiver?.cancel()
        }
    }

    private func waitUntilReady(before deadline: Date) -> Bool {
        condition.withLock {
            while !ready && !failed && !cancelled {
                guard condition.wait(until: deadline) else { return false }
            }
            return ready && !failed && !cancelled
        }
    }

    private func update(_ state: NetworkConnection<TCP>.State) {
        switch state {
        case .ready:
            condition.withLock {
                ready = true
                condition.broadcast()
            }
        case .waiting(let error):
            logger.info("connection waiting: \(error)")
        case .failed(let error):
            fail(error)
        case .cancelled:
            fail(nil)
        default:
            break
        }
    }

    private func deliver(_ content: Data, endOfStream: Bool) {
        condition.withLock {
            buffer.append(content)
            if endOfStream {
                peerFinished = true
            }
            condition.broadcast()
        }
    }

    private func fail(_ error: (any Error)?) {
        condition.withLock {
            guard !cancelled && !peerFinished && !failed else { return }
            if let error {
                logger.error("connection: \(error)")
            }
            failed = true
            condition.broadcast()
        }
    }
}
