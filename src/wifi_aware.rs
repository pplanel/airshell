//! Wi-Fi Aware connections (iOS/iPadOS only) through the Swift wrapper in
//! `swift/AirshellWiFiAware`, which provides the `airshell_wa_*` symbols. The
//! iOS app that links this crate must also link that package.

use std::ffi::{CStr, c_char};
use std::fmt;
use std::ptr::{self, NonNull};
use std::time::Duration;

use crate::transport::Transport;

/// Wi-Fi Aware service airshell publishes and subscribes to. The app must
/// declare it under `WiFiAwareServices` in its Info.plist.
pub const SERVICE: &CStr = c"_airshell._tcp";

#[repr(C)]
struct RawListener {
    _opaque: [u8; 0],
}

#[repr(C)]
struct RawConnection {
    _opaque: [u8; 0],
}

// Mirrors swift/AirshellWiFiAware/Sources/AirshellWiFiAware/Exports.swift.
unsafe extern "C" {
    fn airshell_wa_listen(service: *const c_char) -> *mut RawListener;
    fn airshell_wa_accept(listener: *mut RawListener) -> *mut RawConnection;
    fn airshell_wa_listener_close(listener: *mut RawListener);
    fn airshell_wa_connect(
        service: *const c_char,
        device_name: *const c_char,
        timeout_ms: u32,
    ) -> *mut RawConnection;
    fn airshell_wa_receive(
        connection: *mut RawConnection,
        buffer: *mut u8,
        capacity: usize,
    ) -> isize;
    fn airshell_wa_send(connection: *mut RawConnection, bytes: *const u8, length: usize) -> i32;
    fn airshell_wa_cancel(connection: *mut RawConnection);
    fn airshell_wa_release(connection: *mut RawConnection);
}

/// A Wi-Fi Aware call failed. The Swift side logs why (subsystem `airshell`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WifiAwareError;

impl fmt::Display for WifiAwareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Wi-Fi Aware call failed (see the airshell log)")
    }
}

impl std::error::Error for WifiAwareError {}

/// Publishes a service to all paired devices.
pub struct WifiAwareListener(NonNull<RawListener>);

// SAFETY: the Swift listener synchronizes its own state.
unsafe impl Send for WifiAwareListener {}
unsafe impl Sync for WifiAwareListener {}

impl WifiAwareListener {
    pub fn bind(service: &CStr) -> Result<Self, WifiAwareError> {
        // SAFETY: `service` is a valid C string for the duration of the call.
        let raw = unsafe { airshell_wa_listen(service.as_ptr()) };
        NonNull::new(raw).map(Self).ok_or(WifiAwareError)
    }

    /// Block until a paired device connects.
    pub fn accept(&self) -> Result<WifiAwareConnection, WifiAwareError> {
        // SAFETY: the handle stays valid until `drop`.
        let raw = unsafe { airshell_wa_accept(self.0.as_ptr()) };
        NonNull::new(raw)
            .map(WifiAwareConnection)
            .ok_or(WifiAwareError)
    }
}

impl Drop for WifiAwareListener {
    fn drop(&mut self) {
        // SAFETY: releases the handle exactly once.
        unsafe { airshell_wa_listener_close(self.0.as_ptr()) };
    }
}

/// A TCP connection to a paired device over Wi-Fi Aware.
pub struct WifiAwareConnection(NonNull<RawConnection>);

// SAFETY: the Swift connection synchronizes its own state, and `cancel`
// is meant to be called while another thread is blocked in `receive`.
unsafe impl Send for WifiAwareConnection {}
unsafe impl Sync for WifiAwareConnection {}

impl WifiAwareConnection {
    /// Connect to a paired device publishing `service`: the one named
    /// `device_name`, or the first found.
    pub fn connect(
        service: &CStr,
        device_name: Option<&CStr>,
        timeout: Duration,
    ) -> Result<Self, WifiAwareError> {
        let device_name = device_name.map_or(ptr::null(), CStr::as_ptr);
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: both strings are valid (or null) for the duration of the call.
        let raw = unsafe { airshell_wa_connect(service.as_ptr(), device_name, timeout_ms) };
        NonNull::new(raw).map(Self).ok_or(WifiAwareError)
    }
}

impl Transport for WifiAwareConnection {
    type Error = WifiAwareError;

    fn receive(&self, max_len: usize) -> Result<Vec<u8>, WifiAwareError> {
        let mut buffer = Vec::with_capacity(max_len);
        // SAFETY: the Swift side writes at most `max_len` bytes into `buffer`.
        let received =
            unsafe { airshell_wa_receive(self.0.as_ptr(), buffer.as_mut_ptr(), max_len) };
        let received = usize::try_from(received).map_err(|_| WifiAwareError)?;
        // SAFETY: the first `received` bytes were initialized above.
        unsafe { buffer.set_len(received) };
        Ok(buffer)
    }

    fn send(&self, data: &[u8]) -> Result<(), WifiAwareError> {
        // SAFETY: `data` is valid for reads of `data.len()` bytes.
        match unsafe { airshell_wa_send(self.0.as_ptr(), data.as_ptr(), data.len()) } {
            0 => Ok(()),
            _ => Err(WifiAwareError),
        }
    }

    fn cancel(&self) {
        // SAFETY: the handle stays valid until `drop`.
        unsafe { airshell_wa_cancel(self.0.as_ptr()) };
    }
}

impl Drop for WifiAwareConnection {
    fn drop(&mut self) {
        // SAFETY: releases the handle exactly once.
        unsafe { airshell_wa_release(self.0.as_ptr()) };
    }
}
