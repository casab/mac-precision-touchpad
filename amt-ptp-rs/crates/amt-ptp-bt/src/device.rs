//! Device context and WDF type info for the BT HID filter driver.
//!
//! The device context holds all per-device state: VHF handle, device
//! identification, PTP reporting flags, and timing. This is the BT driver
//! equivalent of the USB driver's `device.rs`.
//!
//! Key difference from USB: no USB handles or Wellspring mode. Instead,
//! a VHF handle creates a virtual PTP device, and an I/O target
//! communicates with the underlying BT HID device.

use core::ffi::c_void;

use wdk_sys::*;

use amt_ptp_core::device::DeviceConfig;
use vhf_sys::VHFHANDLE;

/// Per-device context for the BT HID filter driver.
///
/// Stored in the WDF device object via `WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE`.
/// Equivalent to `DEVICE_CONTEXT` from the C driver's `Device.h`, but with
/// VHF handle replacing the HIDCLASS detour state.
#[repr(C)]
pub struct DeviceContext {
    // ── WDF/WDM Handles ─────────────────────────────────────────
    /// WDF device handle.
    pub device: WDFDEVICE,
    /// WDM device object (needed for `VHF_CONFIG.DeviceObject`).
    pub wdm_device_object: *mut c_void,

    // ── VHF Virtual PTP Device ──────────────────────────────────
    /// Handle to the VHF virtual HID device (created in SelfManagedIoInit).
    pub vhf_handle: VHFHANDLE,

    // ── Device Identification ────────────────────────────────────
    /// Vendor ID of the underlying Apple trackpad (USB or BT VID).
    pub vendor_id: u16,
    /// Product ID of the underlying Apple trackpad.
    pub product_id: u16,
    /// Device version number.
    pub version_number: u16,

    // ── Device Configuration ────────────────────────────────────
    /// Pointer to the static device config (from `CONFIG_TABLE`).
    pub device_info: Option<&'static DeviceConfig>,
    /// Whether the VHF device is fully configured and operational.
    pub device_configured: bool,

    // ── PTP State ───────────────────────────────────────────────
    /// Whether PTP input reporting is enabled (mode = Windows PTP).
    pub ptp_input_on: bool,
    /// Whether surface (touch) reporting is enabled.
    pub ptp_report_touch: bool,
    /// Whether button reporting is enabled.
    pub ptp_report_button: bool,

    // ── Timing ──────────────────────────────────────────────────
    /// Performance counter frequency (ticks per second), for scan time conversion.
    pub perf_freq: i64,
    /// Performance counter value at the last report, for scan time calculation.
    pub last_report_time: i64,

    // ── VHF Report Gating ──────────────────────────────────────
    /// Whether VHF is ready to accept the next input report.
    /// Set to `true` by `EvtVhfReadyForNextReadReport`, cleared after submission.
    pub vhf_ready: bool,

    // ── HID Transport ────────────────────────────────────────────
    /// I/O target to the underlying BT HID device.
    pub hid_io_target: WDFIOTARGET,
    /// Lookaside list for read request buffers.
    pub hid_read_buffer_lookaside: WDFLOOKASIDE,

    // ── Recovery ──────────────────────────────────────────────────
    /// Timer for multitouch configuration retry (fires after 2 seconds).
    pub recovery_timer: WDFTIMER,
    /// Work item for deferred recovery operations.
    pub recovery_work_item: WDFWORKITEM,
    /// Number of consecutive recovery attempts (reset on success).
    pub recovery_attempts: u32,
}

impl DeviceContext {
    /// Initialize all fields to safe defaults.
    ///
    /// Called after WDF allocates the context memory. WDF/WDM handles are
    /// zeroed (null) and get populated during device initialization.
    ///
    /// # Safety
    ///
    /// All handle fields are initialized to null. They must be populated
    /// by the appropriate WDF callbacks before use.
    pub unsafe fn init_defaults(&mut self) {
        self.device = core::ptr::null_mut();
        self.wdm_device_object = core::ptr::null_mut();
        self.vhf_handle = core::ptr::null_mut();
        self.vendor_id = 0;
        self.product_id = 0;
        self.version_number = 0;
        self.device_info = None;
        self.device_configured = false;
        self.ptp_input_on = false;
        self.ptp_report_touch = true; // enabled by default
        self.ptp_report_button = true; // enabled by default
        self.perf_freq = 0;
        self.last_report_time = 0;
        self.vhf_ready = true;
        self.hid_io_target = core::ptr::null_mut();
        self.hid_read_buffer_lookaside = core::ptr::null_mut();
        self.recovery_timer = core::ptr::null_mut();
        self.recovery_work_item = core::ptr::null_mut();
        self.recovery_attempts = 0;
    }
}

/// Get the device context from a WDFDEVICE handle.
///
/// # Safety
///
/// The device must have been created with a context of type [`DeviceContext`].
/// The returned pointer is valid for the lifetime of the device object.
pub unsafe fn get_device_context(device: WDFDEVICE) -> *mut DeviceContext {
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
        ContextName: b"BtDeviceContext\0".as_ptr().cast(),
        ContextSize: core::mem::size_of::<DeviceContext>(),
        UniqueType: core::ptr::null(),
        EvtDriverGetUniqueContextType: None,
    };
