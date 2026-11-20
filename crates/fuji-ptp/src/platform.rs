//! The camera transport of the OS we are running on, behind one set of names so the
//! app and the CLI don't need `#[cfg]`s:
//!
//! * Windows: WPD MTP pass-through (stock driver, no driver swap)
//! * macOS / Linux: libusb, PTP over the camera's bulk endpoints

#[cfg(windows)]
pub use crate::wpd::{com_init, list_devices, WpdDevice as CameraDevice, WpdTransport as CameraTransport};

#[cfg(not(windows))]
pub use crate::usb::{com_init, list_devices, UsbDevice as CameraDevice, UsbTransport as CameraTransport};

/// Human-readable name of the transport, for diagnostics.
pub const NAME: &str = if cfg!(windows) { "WPD" } else { "libusb" };
