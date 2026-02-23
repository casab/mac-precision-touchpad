//! VHF callback implementations for HID feature reports.
//!
//! These callbacks are registered with [`VHF_CONFIG`](vhf_sys::VHF_CONFIG)
//! and called by VHF when Windows sends GET_FEATURE or SET_FEATURE requests
//! to the virtual PTP device.
//!
//! Equivalent to `PtpFilterGetHidFeatures` and `PtpFilterSetHidFeatures`
//! from the C driver's `Hid.c`, but delivered via VHF callbacks instead
//! of IOCTL dispatch.
//!
//! ## Callbacks Registered
//!
//! - [`evt_vhf_get_feature`] → `EvtVhfAsyncOperationGetFeature`
//! - [`evt_vhf_set_feature`] → `EvtVhfAsyncOperationSetFeature`
//! - [`evt_vhf_ready_for_next_read_report`] → `EvtVhfReadyForNextReadReport`
//! - [`evt_vhf_cleanup`] → `EvtVhfCleanup`

use wdk::println;
use wdk_sys::{STATUS_BUFFER_TOO_SMALL, STATUS_NOT_SUPPORTED, STATUS_SUCCESS};

use vhf_sys::*;

use amt_ptp_core::constants::*;
use amt_ptp_core::hid::DEFAULT_HQA_BLOB;
use amt_ptp_core::ptp::*;

use crate::device::DeviceContext;

/// VHF callback for GET_FEATURE requests.
///
/// Handles:
/// - **Report 0x07** (Device Caps): max contacts = 5, button type = clickpad
/// - **Report 0x08** (HQA): 256-byte certification blob
///
/// # Safety
///
/// Called by VHF with valid context and operation handles. The
/// `vhf_client_context` pointer must point to a valid [`DeviceContext`].
pub unsafe extern "C" fn evt_vhf_get_feature(
    vhf_client_context: PVOID,
    vhf_operation_handle: VHFOPERATIONHANDLE,
    _vhf_operation_context: PVOID,
    hid_transfer_packet: *mut HID_XFER_PACKET,
) {
    let _ctx = unsafe { &*(vhf_client_context as *const DeviceContext) };
    let packet = unsafe { &mut *hid_transfer_packet };

    let status = match packet.reportId {
        REPORTID_DEVICE_CAPS => {
            let report_size = core::mem::size_of::<PtpDeviceCapsReport>();
            if (packet.reportBufferLen as usize) < report_size {
                STATUS_BUFFER_TOO_SMALL
            } else {
                let report = PtpDeviceCapsReport::default_clickpad();
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &report as *const PtpDeviceCapsReport as *const u8,
                        packet.reportBuffer,
                        report_size,
                    );
                }
                STATUS_SUCCESS
            }
        }

        REPORTID_PTPHQA => {
            let report_size = core::mem::size_of::<PtpHqaCertificationReport>();
            if (packet.reportBufferLen as usize) < report_size {
                STATUS_BUFFER_TOO_SMALL
            } else {
                let report = PtpHqaCertificationReport {
                    report_id: REPORTID_PTPHQA,
                    blob: DEFAULT_HQA_BLOB,
                };
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &report as *const PtpHqaCertificationReport as *const u8,
                        packet.reportBuffer,
                        report_size,
                    );
                }
                STATUS_SUCCESS
            }
        }

        other => {
            println!("VHF GetFeature: unsupported report ID {other:#x}");
            STATUS_NOT_SUPPORTED
        }
    };

    // Complete the async operation with the result status
    unsafe {
        VhfAsyncOperationComplete(vhf_operation_handle, status);
    }
}

/// VHF callback for SET_FEATURE requests.
///
/// Handles:
/// - **Report 0x04** (Input Mode): switch to Windows PTP mode
/// - **Report 0x06** (Selective Reporting): enable/disable button + surface
///
/// # Safety
///
/// Called by VHF with valid context and operation handles. The
/// `vhf_client_context` pointer must point to a valid [`DeviceContext`].
pub unsafe extern "C" fn evt_vhf_set_feature(
    vhf_client_context: PVOID,
    vhf_operation_handle: VHFOPERATIONHANDLE,
    _vhf_operation_context: PVOID,
    hid_transfer_packet: *mut HID_XFER_PACKET,
) {
    let ctx = unsafe { &mut *(vhf_client_context as *mut DeviceContext) };
    let packet = unsafe { &*hid_transfer_packet };

    let status = match packet.reportId {
        REPORTID_INPUT_MODE => {
            if (packet.reportBufferLen as usize)
                < core::mem::size_of::<PtpInputModeReport>()
            {
                STATUS_BUFFER_TOO_SMALL
            } else {
                let report =
                    unsafe { &*(packet.reportBuffer as *const PtpInputModeReport) };
                let mode = { report.mode }; // copy from packed field
                if mode == PTP_COLLECTION_WINDOWS {
                    ctx.ptp_input_on = true;
                    println!("VHF SetFeature: PTP input mode ON");
                } else {
                    ctx.ptp_input_on = false;
                    println!("VHF SetFeature: PTP input mode OFF (mode={mode})");
                }
                STATUS_SUCCESS
            }
        }

        REPORTID_FUNC_SWITCH => {
            if (packet.reportBufferLen as usize)
                < core::mem::size_of::<PtpSelectiveReportingReport>()
            {
                STATUS_BUFFER_TOO_SMALL
            } else {
                let report = unsafe {
                    &*(packet.reportBuffer as *const PtpSelectiveReportingReport)
                };
                ctx.ptp_report_button = report.button_report_on();
                ctx.ptp_report_touch = report.surface_report_on();
                println!(
                    "VHF SelectiveReporting: button={}, surface={}",
                    ctx.ptp_report_button, ctx.ptp_report_touch
                );
                STATUS_SUCCESS
            }
        }

        other => {
            println!("VHF SetFeature: unsupported report ID {other:#x}");
            STATUS_NOT_SUPPORTED
        }
    };

    // Complete the async operation with the result status
    unsafe {
        VhfAsyncOperationComplete(vhf_operation_handle, status);
    }
}

/// VHF callback when ready for the next input report.
///
/// Called by VHF when it has consumed the previous report and is ready
/// for more data. Sets the `vhf_ready` flag so the BT read completion
/// callback knows it can submit the next report.
///
/// # Safety
///
/// Called by VHF with valid client context pointing to [`DeviceContext`].
pub unsafe extern "C" fn evt_vhf_ready_for_next_read_report(
    vhf_client_context: PVOID,
) {
    let ctx = unsafe { &mut *(vhf_client_context as *mut DeviceContext) };
    ctx.vhf_ready = true;
}

/// VHF cleanup callback.
///
/// Called during VHF device deletion. The driver should release any
/// VHF-specific resources here.
///
/// # Safety
///
/// Called by VHF with valid client context.
pub unsafe extern "C" fn evt_vhf_cleanup(_vhf_client_context: PVOID) {
    // No additional cleanup needed — the driver context is managed by WDF.
    // VHF-specific resources (the handle itself) are cleaned up by VhfDelete.
}
