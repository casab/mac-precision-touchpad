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
use crate::recovery;
use crate::transport;
use crate::vhf_device;

/// EvtDeviceSelfManagedIoInit — called once at first D0 entry.
///
/// Creates the VHF virtual PTP device with the correct HID report descriptor
/// and registers VHF callbacks for feature reports.
///
/// ## Flow
/// 1. Initialize BT HID transport (I/O target + lookaside list)
/// 2. Query VID/PID from the underlying BT HID device
/// 3. Look up device config and build HID report descriptor
/// 4. Configure VHF with callbacks and device identity
/// 5. Create and start the VHF device
/// 6. Activate multitouch mode on the trackpad (report 0xF1)
/// 7. Issue first read request to start receiving touch data
///
/// # Safety
///
/// Called by WDF with a valid device handle. The device must have been
/// created with a [`DeviceContext`](crate::device::DeviceContext).
pub unsafe extern "C" fn evt_self_managed_io_init(device: WDFDEVICE) -> NTSTATUS {
    let ctx = unsafe { &mut *get_device_context(device) };

    println!("SelfManagedIoInit: initializing BT transport and VHF device");

    // Initialize HID transport (I/O target + lookaside list)
    let status = unsafe { transport::init_transport(device) };
    if !NT_SUCCESS(status) {
        println!("SelfManagedIoInit: init_transport failed: {status:#x}");
        return status;
    }

    // Query VID/PID from the underlying BT HID device
    let status = unsafe { transport::query_device_attributes(ctx) };
    if !NT_SUCCESS(status) {
        println!("SelfManagedIoInit: query_device_attributes failed: {status:#x}");
        // Fall back to hardcoded MT2 values
        ctx.vendor_id = BT_VENDOR_ID_APPLE;
        ctx.product_id = PID_MAGIC_TRACKPAD2;
        ctx.version_number = DEVICE_VERSION as u16;
    }

    // Look up device config by product ID
    let config = lookup_config(ctx.product_id);
    ctx.device_info = Some(config);

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

    // Activate multitouch mode on the BT trackpad (report 0xF1)
    ctx.device_configured = true;
    ctx.vhf_ready = true;
    ctx.recovery_attempts = 0;

    let status = unsafe { transport::activate_multitouch(ctx) };
    if !NT_SUCCESS(status) {
        // Non-fatal: the device may not be ready yet. Schedule recovery.
        println!("SelfManagedIoInit: multitouch activation failed: {status:#x} (scheduling retry)");
        unsafe { recovery::start_recovery_timer(ctx) };
    } else {
        // Issue the first read request to start receiving touch data
        let status = unsafe { transport::issue_read_request(device) };
        if !NT_SUCCESS(status) {
            println!("SelfManagedIoInit: first read request failed: {status:#x} (scheduling retry)");
            unsafe { recovery::start_recovery_timer(ctx) };
        }
    }

    println!(
        "SelfManagedIoInit: VHF virtual PTP device created (PID={:#06x})",
        ctx.product_id
    );
    STATUS_SUCCESS
}

/// EvtDeviceSelfManagedIoRestart — called on subsequent D0 entries (after suspend).
///
/// Re-records the timestamp, restarts the I/O target, re-activates multitouch
/// mode on the BT trackpad, and reissues read requests.
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

    // Restart the I/O target (stopped during suspend)
    if !ctx.hid_io_target.is_null() {
        unsafe {
            call_unsafe_wdf_function_binding!(
                WdfIoTargetStart,
                ctx.hid_io_target
            );
        }
    }

    // Re-activate multitouch mode (power-down may have reset the trackpad)
    ctx.device_configured = true;
    ctx.vhf_ready = true;
    ctx.recovery_attempts = 0;

    let status = unsafe { transport::activate_multitouch(ctx) };
    if !NT_SUCCESS(status) {
        println!("SelfManagedIoRestart: multitouch activation failed: {status:#x} (scheduling retry)");
        unsafe { recovery::start_recovery_timer(ctx) };
    } else {
        // Reissue read request to resume data flow
        let status = unsafe { transport::issue_read_request(device) };
        if !NT_SUCCESS(status) {
            println!("SelfManagedIoRestart: read request failed: {status:#x} (scheduling retry)");
            unsafe { recovery::start_recovery_timer(ctx) };
        }
    }

    println!("SelfManagedIoRestart: device reconfigured");
    STATUS_SUCCESS
}

/// EvtDeviceSelfManagedIoSuspend — called before leaving D0 (power-down).
///
/// Marks the device as not configured and stops the I/O target to cancel
/// all pending BT transport read requests.
///
/// # Safety
///
/// Called by WDF with a valid device handle.
pub unsafe extern "C" fn evt_self_managed_io_suspend(device: WDFDEVICE) -> NTSTATUS {
    let ctx = unsafe { &mut *get_device_context(device) };

    // Mark device as not configured first to prevent read resubmission
    ctx.device_configured = false;

    // Stop the recovery timer if running
    unsafe { recovery::stop_recovery_timer(ctx) };

    // Cancel pending read requests by stopping the I/O target.
    // WdfIoTargetStop with WdfIoTargetCancelSentIo cancels all outstanding requests.
    if !ctx.hid_io_target.is_null() {
        unsafe {
            call_unsafe_wdf_function_binding!(
                WdfIoTargetStop,
                ctx.hid_io_target,
                WDF_IO_TARGET_SENT_IO_ACTION::WdfIoTargetCancelSentIo
            );
        }
    }

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
