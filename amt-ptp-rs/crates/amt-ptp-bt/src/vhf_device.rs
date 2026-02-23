//! Safe wrapper functions for VHF virtual HID device operations.
//!
//! Provides null-checked, ergonomic wrappers around the raw `vhf-sys` FFI
//! functions. The raw `VHFHANDLE` is stored in [`DeviceContext`](crate::device::DeviceContext)
//! and its lifecycle is managed by the self-managed I/O callbacks.
//!
//! ## Lifecycle
//!
//! ```text
//! SelfManagedIoInit:    vhf_create() → vhf_start()
//! (normal operation):   vhf_submit_read_report()
//! SelfManagedIoCleanup: vhf_delete()
//! ```

use vhf_sys::*;
use wdk_sys::NTSTATUS;

/// Create a VHF virtual HID device.
///
/// On success, returns the `VHFHANDLE`. On failure, returns the NTSTATUS error.
///
/// # Safety
///
/// `config` must be a valid, fully initialized [`VHF_CONFIG`] with a valid
/// `DeviceObject` and `ReportDescriptor`. The report descriptor buffer only
/// needs to remain valid for the duration of this call (VHF copies it).
pub unsafe fn vhf_create(config: &mut VHF_CONFIG) -> Result<VHFHANDLE, NTSTATUS> {
    let mut handle: VHFHANDLE = core::ptr::null_mut();
    let status = unsafe { VhfCreate(config as *mut VHF_CONFIG, &mut handle) };
    if status >= 0 {
        // NT_SUCCESS
        Ok(handle)
    } else {
        Err(status)
    }
}

/// Start a VHF virtual HID device, making it visible to Windows.
///
/// After this call, VHF callbacks may be invoked. The device appears in
/// Device Manager as a HID device.
///
/// # Safety
///
/// `handle` must be a valid `VHFHANDLE` from a successful [`vhf_create`].
pub unsafe fn vhf_start(handle: VHFHANDLE) -> NTSTATUS {
    if handle.is_null() {
        return wdk_sys::STATUS_INVALID_HANDLE;
    }
    unsafe { VhfStart(handle) }
}

/// Submit an input (read) report to the VHF virtual HID device.
///
/// VHF forwards the report to the HID class driver as if it came from
/// real hardware. Call this when new touch data is available from the
/// BT transport.
///
/// # Safety
///
/// - `handle` must be a valid, started `VHFHANDLE`
/// - `packet` must contain a valid report buffer with correct length
pub unsafe fn vhf_submit_read_report(
    handle: VHFHANDLE,
    packet: &mut HID_XFER_PACKET,
) -> NTSTATUS {
    if handle.is_null() {
        return wdk_sys::STATUS_INVALID_HANDLE;
    }
    unsafe { VhfReadReportSubmit(handle, packet as *mut HID_XFER_PACKET) }
}

/// Complete an async VHF operation (for feature report callbacks).
///
/// Must be called from VHF async callbacks ([`EVT_VHF_ASYNC_OPERATION`])
/// to signal completion of GET_FEATURE, SET_FEATURE, etc.
///
/// # Safety
///
/// `operation_handle` must be a valid `VHFOPERATIONHANDLE` received in a
/// VHF callback.
pub unsafe fn vhf_async_operation_complete(
    operation_handle: VHFOPERATIONHANDLE,
    status: NTSTATUS,
) -> NTSTATUS {
    unsafe { VhfAsyncOperationComplete(operation_handle, status) }
}

/// Delete a VHF virtual HID device.
///
/// If `wait` is true, blocks until all pending operations complete.
/// The handle becomes invalid after this call and must not be used.
///
/// Safe to call with a null handle (no-op).
///
/// # Safety
///
/// If non-null, `handle` must be a valid `VHFHANDLE`.
pub unsafe fn vhf_delete(handle: VHFHANDLE, wait: bool) {
    if !handle.is_null() {
        unsafe { VhfDelete(handle, if wait { 1 } else { 0 }) };
    }
}
