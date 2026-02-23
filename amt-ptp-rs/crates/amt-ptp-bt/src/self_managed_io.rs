//! Self-managed I/O lifecycle callbacks for VHF device management.
//!
//! WDF self-managed I/O callbacks provide a clean lifecycle for resources
//! that need to be created once (at first D0 entry) and cleaned up at
//! device removal:
//!
//! - **Init** (once): Create and start the VHF virtual PTP device
//! - **Restart** (subsequent D0): Re-configure multitouch, update timing
//! - **Suspend** (leaving D0): Mark device as not configured
//! - **Cleanup** (removal): Delete the VHF device
//!
//! This replaces the C driver's HIDCLASS detour (`PtpFilterDetourWindowsHIDStack`)
//! with Microsoft's documented Virtual HID Framework (VHF) API.

extern crate alloc;

use wdk::println;
use wdk_sys::*;

use amt_ptp_core::constants::*;
use amt_ptp_core::device::lookup_config;
use amt_ptp_core::hid::build_report_descriptor;

use crate::device::get_device_context;
use crate::hid;
use crate::vhf_device;

/// EvtDeviceSelfManagedIoInit — called once at first D0 entry.
///
/// Creates the VHF virtual PTP device with the correct HID report descriptor
/// and registers VHF callbacks for feature reports.
///
/// ## Flow
/// 1. Look up device config (MT2 hardcoded for Phase 7)
/// 2. Build HID report descriptor from device config
/// 3. Configure VHF with callbacks and device identity
/// 4. Create and start the VHF device
/// 5. Record initial timestamp for scan time calculation
///
/// # Safety
///
/// Called by WDF with a valid device handle. The device must have been
/// created with a [`DeviceContext`](crate::device::DeviceContext).
pub unsafe extern "C" fn evt_self_managed_io_init(device: WDFDEVICE) -> NTSTATUS {
    let ctx = unsafe { &mut *get_device_context(device) };

    println!("SelfManagedIoInit: creating VHF virtual PTP device");

    // Phase 7: Use hardcoded MT2 config (the INX only matches MT2).
    // Phase 8 will query VID/PID from the underlying HID device via I/O target.
    let config = lookup_config(PID_MAGIC_TRACKPAD2);
    ctx.device_info = Some(config);
    ctx.product_id = PID_MAGIC_TRACKPAD2;
    ctx.vendor_id = BT_VENDOR_ID_APPLE;
    ctx.version_number = DEVICE_VERSION as u16;

    // Build the HID report descriptor for this device
    let report_desc = build_report_descriptor(
        config.ptp_x_logical_max(),
        config.ptp_y_logical_max(),
        config.ptp_x_physical_max(),
        config.ptp_y_physical_max(),
    );

    // Create VHF_CONFIG with required fields
    // SAFETY: wdm_device_object was set in EvtDriverDeviceAdd and is valid.
    // report_desc is a valid buffer; VHF copies it during VhfCreate.
    let mut vhf_config = unsafe {
        vhf_sys::VHF_CONFIG::init(
            ctx.wdm_device_object,
            report_desc.as_ptr() as vhf_sys::PUCHAR,
            report_desc.len() as vhf_sys::USHORT,
        )
    };

    // Set device identification for the virtual PTP device.
    // Use DEVICE_VID (0x8910) instead of Apple's VID to avoid conflicts.
    vhf_config.VendorID = DEVICE_VID;
    vhf_config.ProductID = ctx.product_id;
    vhf_config.VersionNumber = ctx.version_number;

    // Set client context (pointer to our DeviceContext for callbacks)
    vhf_config.VhfClientContext = ctx as *mut _ as vhf_sys::PVOID;

    // Register VHF callbacks
    vhf_config.EvtVhfAsyncOperationGetFeature = Some(hid::evt_vhf_get_feature);
    vhf_config.EvtVhfAsyncOperationSetFeature = Some(hid::evt_vhf_set_feature);
    vhf_config.EvtVhfReadyForNextReadReport =
        Some(hid::evt_vhf_ready_for_next_read_report);
    vhf_config.EvtVhfCleanup = Some(hid::evt_vhf_cleanup);

    // Create the VHF device
    match unsafe { vhf_device::vhf_create(&mut vhf_config) } {
        Ok(handle) => ctx.vhf_handle = handle,
        Err(status) => {
            println!("SelfManagedIoInit: VhfCreate failed: {status:#x}");
            return status;
        }
    }

    // Start the VHF device (makes it visible to Windows as a HID device)
    let status = unsafe { vhf_device::vhf_start(ctx.vhf_handle) };
    if !NT_SUCCESS(status) {
        println!("SelfManagedIoInit: VhfStart failed: {status:#x}");
        // Clean up the created-but-not-started device
        unsafe { vhf_device::vhf_delete(ctx.vhf_handle, true) };
        ctx.vhf_handle = core::ptr::null_mut();
        return status;
    }

    // Record initial timestamp for scan time calculation.
    // SAFETY: KeQueryPerformanceCounter is always safe to call.
    let mut freq: LARGE_INTEGER = unsafe { core::mem::zeroed() };
    let counter = unsafe { KeQueryPerformanceCounter(&mut freq) };
    ctx.perf_freq = unsafe { *freq.QuadPart() };
    ctx.last_report_time = unsafe { *counter.QuadPart() };

    ctx.device_configured = true;

    println!(
        "SelfManagedIoInit: VHF virtual PTP device created (PID={:#06x})",
        ctx.product_id
    );
    STATUS_SUCCESS
}

/// EvtDeviceSelfManagedIoRestart — called on subsequent D0 entries (after suspend).
///
/// Re-records the timestamp for scan time calculation and restores device state.
/// Phase 8 will also re-configure multitouch on the underlying BT device here.
///
/// # Safety
///
/// Called by WDF with a valid device handle.
pub unsafe extern "C" fn evt_self_managed_io_restart(device: WDFDEVICE) -> NTSTATUS {
    let ctx = unsafe { &mut *get_device_context(device) };

    // Record fresh timestamp for scan time calculation
    let mut freq: LARGE_INTEGER = unsafe { core::mem::zeroed() };
    let counter = unsafe { KeQueryPerformanceCounter(&mut freq) };
    ctx.perf_freq = unsafe { *freq.QuadPart() };
    ctx.last_report_time = unsafe { *counter.QuadPart() };

    // Phase 8: re-send multitouch activation command (0xF1) to the
    // underlying BT HID device, since power-down may have reset it.

    ctx.device_configured = true;
    println!("SelfManagedIoRestart: device reconfigured");

    STATUS_SUCCESS
}

/// EvtDeviceSelfManagedIoSuspend — called before leaving D0 (power-down).
///
/// Marks the device as not configured. Phase 8 will also stop pending
/// read requests here.
///
/// # Safety
///
/// Called by WDF with a valid device handle.
pub unsafe extern "C" fn evt_self_managed_io_suspend(device: WDFDEVICE) -> NTSTATUS {
    let ctx = unsafe { &mut *get_device_context(device) };

    ctx.device_configured = false;

    // Phase 8: cancel pending read requests from the BT transport

    STATUS_SUCCESS
}

/// EvtDeviceSelfManagedIoCleanup — called during device removal.
///
/// Deletes the VHF virtual PTP device, waiting for all pending
/// operations to complete.
///
/// # Safety
///
/// Called by WDF with a valid device handle. After this call, the
/// VHF handle is invalid.
pub unsafe extern "C" fn evt_self_managed_io_cleanup(device: WDFDEVICE) {
    let ctx = unsafe { &mut *get_device_context(device) };

    // Delete the VHF device, blocking until all pending operations complete
    if !ctx.vhf_handle.is_null() {
        println!("SelfManagedIoCleanup: deleting VHF device");
        unsafe { vhf_device::vhf_delete(ctx.vhf_handle, true) };
        ctx.vhf_handle = core::ptr::null_mut();
    }

    ctx.device_configured = false;
}
