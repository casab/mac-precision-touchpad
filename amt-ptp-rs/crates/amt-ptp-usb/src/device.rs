//! Device context and WDF device creation.
//!
//! The device context holds all per-device state: USB handles, device config,
//! Wellspring mode state, PTP reporting flags, and timing.

use core::sync::atomic::{AtomicBool, Ordering};

use wdk_sys::*;

use amt_ptp_core::device::DeviceConfig;

/// Per-device context stored in the WDF device object.
///
/// This is the Rust equivalent of `DEVICE_CONTEXT` from the C driver's `Device.h`.
/// It is allocated by WDF alongside the device object via
/// `WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE`.
#[repr(C)]
pub struct DeviceContext {
    // ── USB Handles ─────────────────────────────────────────────
    /// WDF USB device handle (created in PrepareHardware).
    pub usb_device: WDFUSBDEVICE,
    /// USB interrupt pipe for trackpad touch data.
    pub interrupt_pipe: WDFUSBPIPE,
    /// Selected USB interface.
    pub usb_interface: WDFUSBINTERFACE,
    /// USB device descriptor (VID, PID, etc).
    pub device_descriptor: USB_DEVICE_DESCRIPTOR,
    /// USB device traits (high-speed, self-powered, etc).
    pub usb_device_traits: ULONG,

    // ── Queues ──────────────────────────────────────────────────
    /// Manual-dispatch queue for pending HID read report requests.
    /// The continuous reader callback dequeues from here.
    pub input_queue: WDFQUEUE,

    // ── Device Configuration ────────────────────────────────────
    /// Pointer to the static device config (from `CONFIG_TABLE`).
    pub device_info: Option<&'static DeviceConfig>,
    /// Whether Wellspring mode (raw multitouch data) is active.
    pub is_wellspring_mode_on: bool,

    // ── PTP State ───────────────────────────────────────────────
    /// Whether PTP input reporting is enabled (mode = Windows PTP).
    pub ptp_input_on: AtomicBool,
    /// Whether surface (touch) reporting is enabled.
    pub ptp_report_touch: AtomicBool,
    /// Whether button reporting is enabled.
    pub ptp_report_button: AtomicBool,

    // ── Timing ──────────────────────────────────────────────────
    /// Performance counter frequency (ticks per second), for scan time conversion.
    pub perf_freq: i64,
    /// Performance counter value at the last report, for scan time calculation.
    pub last_report_time: i64,
}

impl DeviceContext {
    /// Initialize a device context with default values.
    ///
    /// Called after WDF allocates the context memory. USB handles are
    /// zeroed (null) and get filled in during `PrepareHardware`.
    ///
    /// # Safety
    ///
    /// All WDF handle fields are initialized to null (zero). They must be
    /// populated by the appropriate WDF calls before use.
    pub unsafe fn init_defaults(&mut self) {
        self.usb_device = core::ptr::null_mut();
        self.interrupt_pipe = core::ptr::null_mut();
        self.usb_interface = core::ptr::null_mut();
        self.device_descriptor = core::mem::zeroed();
        self.usb_device_traits = 0;
        self.input_queue = core::ptr::null_mut();
        self.device_info = None;
        self.is_wellspring_mode_on = false;
        self.ptp_input_on = AtomicBool::new(false);
        self.ptp_report_touch = AtomicBool::new(true); // enabled by default
        self.ptp_report_button = AtomicBool::new(true); // enabled by default
        self.perf_freq = 0;
        self.last_report_time = 0;
    }
}

// WDF context type accessor macro equivalent.
// In the C driver this is `WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(DEVICE_CONTEXT, DeviceGetContext)`.
// With wdk-sys we use `wdf_device_context!` or manual implementation.

/// Get the device context from a WDFDEVICE handle.
///
/// # Safety
///
/// The device must have been created with a context of type [`DeviceContext`].
/// The returned pointer is valid for the lifetime of the device object.
pub unsafe fn get_device_context(device: WDFDEVICE) -> *mut DeviceContext {
    // SAFETY: WdfObjectGetTypedContext returns the context pointer that was
    // allocated alongside the device object. The type and size were set during
    // WdfDeviceCreate via WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE.
    unsafe {
        WdfObjectGetTypedContext(
            device.cast(),
            &DEVICE_CONTEXT_TYPE_INFO as *const WDF_OBJECT_CONTEXT_TYPE_INFO,
        )
        .cast::<DeviceContext>()
    }
}

/// WDF context type info for [`DeviceContext`].
///
/// This is the static type descriptor that WDF uses to track context size
/// and type. It's the equivalent of what `WDF_DECLARE_CONTEXT_TYPE` generates.
#[used]
pub static DEVICE_CONTEXT_TYPE_INFO: WDF_OBJECT_CONTEXT_TYPE_INFO =
    WDF_OBJECT_CONTEXT_TYPE_INFO {
        Size: core::mem::size_of::<WDF_OBJECT_CONTEXT_TYPE_INFO>() as ULONG,
        ContextName: b"DeviceContext\0".as_ptr().cast(),
        ContextSize: core::mem::size_of::<DeviceContext>(),
        UniqueType: core::ptr::null(),
        EvtDriverGetUniqueContextType: None,
    };
