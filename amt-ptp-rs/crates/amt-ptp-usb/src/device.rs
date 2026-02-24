//! Device context and WDF device creation.
//!
//! The device context holds all per-device state: USB handles, device config,
//! Wellspring mode state, PTP reporting flags, and timing.

use core::sync::atomic::AtomicBool;

use wdk_sys::*;

use amt_ptp_core::device::DeviceConfig;

// Compile-time check: Option<&DeviceConfig> must be pointer-sized for repr(C) layout.
// Rust guarantees this for Option<&T> (nullable pointer optimization), but a static
// assert protects against any hypothetical future change.
const _: () = assert!(
    core::mem::size_of::<Option<&DeviceConfig>>() == core::mem::size_of::<*const DeviceConfig>(),
    "Option<&DeviceConfig> must be pointer-sized for repr(C) DeviceContext"
);

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
        // SAFETY: USB_DEVICE_DESCRIPTOR is a plain C struct, safe to zero-init
        self.device_descriptor = unsafe { core::mem::zeroed() };
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

// ── Sync wrapper for WDF context type info ────────────────────────────
//
// WDF_OBJECT_CONTEXT_TYPE_INFO contains raw pointers (*const i8 for ContextName,
// *const Self for UniqueType) which don't implement Sync. We use a newtype
// wrapper to provide the Sync impl needed for a `static`.

/// Wrapper to allow `WDF_OBJECT_CONTEXT_TYPE_INFO` in a `static`.
#[repr(transparent)]
pub struct SyncContextTypeInfo(pub WDF_OBJECT_CONTEXT_TYPE_INFO);

// SAFETY: WDF_OBJECT_CONTEXT_TYPE_INFO is read-only after initialization.
// It contains raw pointers to static data (string literals and null) and
// an Option<fn> callback. WDF accesses this from any thread context,
// matching the behavior of the C driver's WDF_DECLARE_CONTEXT_TYPE.
unsafe impl Sync for SyncContextTypeInfo {}

/// Get the device context from a WDFDEVICE handle.
///
/// # Safety
///
/// The device must have been created with a context of type [`DeviceContext`].
/// The returned pointer is valid for the lifetime of the device object.
pub unsafe fn get_device_context(device: WDFDEVICE) -> *mut DeviceContext {
    // SAFETY: WdfObjectGetTypedContextWorker returns the context pointer that was
    // allocated alongside the device object. The type and size were set during
    // WdfDeviceCreate via WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE.
    unsafe {
        call_unsafe_wdf_function_binding!(
            WdfObjectGetTypedContextWorker,
            device.cast(),
            &DEVICE_CONTEXT_TYPE_INFO.0 as *const WDF_OBJECT_CONTEXT_TYPE_INFO,
        )
        .cast::<DeviceContext>()
    }
}

/// WDF context type info for [`DeviceContext`].
///
/// This is the static type descriptor that WDF uses to track context size
/// and type. It's the equivalent of what `WDF_DECLARE_CONTEXT_TYPE` generates.
#[used]
pub static DEVICE_CONTEXT_TYPE_INFO: SyncContextTypeInfo =
    SyncContextTypeInfo(WDF_OBJECT_CONTEXT_TYPE_INFO {
        Size: core::mem::size_of::<WDF_OBJECT_CONTEXT_TYPE_INFO>() as ULONG,
        ContextName: b"DeviceContext\0".as_ptr().cast(),
        ContextSize: core::mem::size_of::<DeviceContext>(),
        UniqueType: core::ptr::null(),
        EvtDriverGetUniqueContextType: None,
    });
