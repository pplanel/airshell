// The C ABI mirrored by `src/wifi_aware.rs`. Every function blocks the calling
// thread, so call them from Rust threads, never from Swift concurrency.
// Handles are retained objects: release each exactly once.

import Foundation
import OSLog
import WiFiAware

let logger = Logger(subsystem: "airshell", category: "wifi-aware")

/// Publish `service` (declared under `WiFiAwareServices` in Info.plist) to all
/// paired devices. Null if the service is not declared as publishable.
@_cdecl("airshell_wa_listen")
public func airshell_wa_listen(_ service: UnsafePointer<CChar>) -> UnsafeMutableRawPointer? {
    let name = String(cString: service)
    guard let service = WAPublishableService.allServices[name] else {
        logger.error("listen: \(name) is not a publishable service in Info.plist")
        return nil
    }
    return Unmanaged.passRetained(Listener(publishing: service)).toOpaque()
}

/// Block until a paired device connects. Null once the listener has failed.
@_cdecl("airshell_wa_accept")
public func airshell_wa_accept(_ listener: UnsafeMutableRawPointer) -> UnsafeMutableRawPointer? {
    let connection = Unmanaged<Listener>.fromOpaque(listener).takeUnretainedValue().accept()
    return connection.map { Unmanaged.passRetained($0).toOpaque() }
}

/// Stop publishing and release the listener handle.
@_cdecl("airshell_wa_listener_close")
public func airshell_wa_listener_close(_ listener: UnsafeMutableRawPointer) {
    let listener = Unmanaged<Listener>.fromOpaque(listener)
    listener.takeUnretainedValue().close()
    listener.release()
}

/// Connect to a paired device publishing `service`: the one named
/// `deviceName`, or the first found when it is null. Null on failure or timeout.
@_cdecl("airshell_wa_connect")
public func airshell_wa_connect(
    _ service: UnsafePointer<CChar>,
    _ deviceName: UnsafePointer<CChar>?,
    _ timeoutMilliseconds: UInt32
) -> UnsafeMutableRawPointer? {
    let name = String(cString: service)
    guard let service = WASubscribableService.allServices[name] else {
        logger.error("connect: \(name) is not a subscribable service in Info.plist")
        return nil
    }
    let connection = Connection.connect(
        to: service,
        deviceName: deviceName.map { String(cString: $0) },
        timeout: Double(timeoutMilliseconds) / 1000
    )
    return connection.map { Unmanaged.passRetained($0).toOpaque() }
}

/// Receive up to `capacity` bytes into `buffer`. Returns the byte count, 0 at
/// end of stream, or -1 on failure or cancellation.
@_cdecl("airshell_wa_receive")
public func airshell_wa_receive(
    _ connection: UnsafeMutableRawPointer,
    _ buffer: UnsafeMutablePointer<UInt8>,
    _ capacity: UInt
) -> Int {
    guard let data = Unmanaged<Connection>.fromOpaque(connection).takeUnretainedValue().receive(atMost: Int(capacity))
    else { return -1 }
    data.copyBytes(to: buffer, count: data.count)
    return data.count
}

/// Send `length` bytes from `bytes`. Returns 0, or -1 on failure or cancellation.
@_cdecl("airshell_wa_send")
public func airshell_wa_send(
    _ connection: UnsafeMutableRawPointer,
    _ bytes: UnsafePointer<UInt8>,
    _ length: UInt
) -> Int32 {
    let data = Data(bytes: bytes, count: Int(length))
    return Unmanaged<Connection>.fromOpaque(connection).takeUnretainedValue().send(data) ? 0 : -1
}

/// Close gracefully (FIN) and unblock pending calls on other threads. Idempotent.
@_cdecl("airshell_wa_cancel")
public func airshell_wa_cancel(_ connection: UnsafeMutableRawPointer) {
    Unmanaged<Connection>.fromOpaque(connection).takeUnretainedValue().cancel()
}

/// Cancel the connection and release its handle.
@_cdecl("airshell_wa_release")
public func airshell_wa_release(_ connection: UnsafeMutableRawPointer) {
    let connection = Unmanaged<Connection>.fromOpaque(connection)
    connection.takeUnretainedValue().cancel()
    connection.release()
}
