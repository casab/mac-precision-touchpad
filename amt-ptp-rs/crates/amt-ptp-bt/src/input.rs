//! BT HID read completion callback — raw touch data → PTP report → VHF submission.
//!
//! When the Bluetooth HID transport delivers a raw touch report, our completion
//! callback parses Apple TYPE5 finger data, builds a Windows PTP input report,
//! and submits it to VHF via `VhfReadReportSubmit`. Then it reissues the next
//! read request to keep the data flowing.
//!
//! This is the BT equivalent of `input.rs` in the USB driver, with two key
//! differences:
//! - Output goes to VHF (`vhf_submit_read_report`) instead of a pending HID request
//! - Read requests are manually resubmitted (no USB continuous reader)
//!
//! Ported from `Input.c` (`PtpFilterInputRequestCompletionCallback`) in the
//! C BT filter driver.

use core::sync::atomic::Ordering;

use wdk_sys::*;

use crate::device::get_device_context;
use crate::recovery;
use crate::transport;
use crate::vhf_device;

use amt_ptp_core::constants::PTP_MAX_CONTACT_POINTS;
use amt_ptp_core::finger::{parse_report, Finger};
use amt_ptp_core::ptp::{PtpContact, PtpReport};

use vhf_sys::HID_XFER_PACKET;

/// BT HID read request completion callback.
///
/// Called by WDF when the lower BT HID device completes an
/// `IOCTL_HID_READ_REPORT` request. Steps:
/// 1. Extract the raw report bytes from the completed request
/// 2. Parse Apple TYPE5 finger data using the BT header size (4 bytes)
/// 3. Calculate scan time from performance counter delta
/// 4. Build a PTP input report with up to 5 contacts
/// 5. Submit the report to VHF via `VhfReadReportSubmit`
/// 6. Reissue the next read request to keep data flowing
///
/// # Safety
///
/// Called by WDF with valid request, I/O target, and context (WDFDEVICE).
pub unsafe extern "C" fn evt_bt_read_complete(
    request: WDFREQUEST,
    _target: WDFIOTARGET,
    _params: PWDF_REQUEST_COMPLETION_PARAMS,
    context: WDFCONTEXT,
) {
    let device: WDFDEVICE = context.cast();
    // SAFETY: device was created with DeviceContext
    let ctx = unsafe { get_device_context(device) };

    // Check request completion status
    let status = unsafe {
        call_unsafe_wdf_function_binding!(WdfRequestGetStatus, request)
    };
    if !NT_SUCCESS(status) {
        // Read failed — clean up and schedule recovery
        unsafe {
            call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
        }
        if unsafe { (*ctx).device_configured.load(Ordering::Relaxed) } {
            unsafe { recovery::start_recovery_timer(&*ctx) };
        }
        return;
    }

    let config = match unsafe { (*ctx).device_info } {
        Some(c) => c,
        None => {
            unsafe {
                call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
            }
            return;
        }
    };

    // Get bytes transferred
    let bytes_returned = unsafe {
        call_unsafe_wdf_function_binding!(WdfRequestGetInformation, request)
    } as usize;

    // Skip if no data or PTP input mode is not active
    if bytes_returned == 0 || !unsafe { (*ctx).ptp_input_on.load(Ordering::Relaxed) } {
        unsafe {
            call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
        }
        if unsafe { (*ctx).device_configured.load(Ordering::Relaxed) } {
            let _ = unsafe { transport::issue_read_request(device) };
        }
        return;
    }

    // Retrieve the output memory from the completed request
    let mut output_mem: WDFMEMORY = core::ptr::null_mut();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestRetrieveOutputMemory,
            request,
            &mut output_mem
        )
    };
    if !NT_SUCCESS(status) || output_mem.is_null() {
        unsafe {
            call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
        }
        if unsafe { (*ctx).device_configured.load(Ordering::Relaxed) } {
            let _ = unsafe { transport::issue_read_request(device) };
        }
        return;
    }

    // Get the raw buffer pointer from the memory object
    let raw_buf_ptr = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfMemoryGetBuffer,
            output_mem,
            core::ptr::null_mut()
        )
    };
    if raw_buf_ptr.is_null() {
        unsafe {
            call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
        }
        if unsafe { (*ctx).device_configured.load(Ordering::Relaxed) } {
            let _ = unsafe { transport::issue_read_request(device) };
        }
        return;
    }

    let raw_data = unsafe {
        core::slice::from_raw_parts(raw_buf_ptr as *const u8, bytes_returned)
    };

    // ── Parse finger data (BT header = 4 bytes for TYPE5) ────────────
    let header_size = config.trackpad_type.header_size_bt();
    let mut fingers = [Finger {
        raw_x: 0,
        raw_y: 0,
        touch_major: 0,
        touch_minor: 0,
        size: 0,
        pressure: 0,
        contact_id: 0,
        orientation: 0,
    }; PTP_MAX_CONTACT_POINTS];

    let (count, button) = match parse_report(raw_data, config, header_size, &mut fingers) {
        Ok(result) => result,
        Err(_) => {
            // Malformed data — may indicate device not in multitouch mode.
            // Schedule recovery to re-activate and resubmit.
            unsafe {
                call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
            }
            if unsafe { (*ctx).device_configured.load(Ordering::Relaxed) } {
                unsafe { recovery::start_recovery_timer(&*ctx) };
            }
            return;
        }
    };

    // Defense-in-depth: clamp count to PTP max even though parse_report already does
    let count = count.min(PTP_MAX_CONTACT_POINTS);

    // ── Calculate scan time ──────────────────────────────────────────
    // PTP scan time is in 100µs units.
    let perf_counter = unsafe { KeQueryPerformanceCounter(core::ptr::null_mut()) };
    let current_time = unsafe { *perf_counter.QuadPart() };
    let delta_ticks = (current_time - unsafe { (*ctx).last_report_time }).max(0);
    let delta = if unsafe { (*ctx).perf_freq } > 0 {
        delta_ticks.saturating_mul(10_000) / unsafe { (*ctx).perf_freq }
    } else {
        delta_ticks / 1000
    };
    let scan_time = if delta > 0xFFFF { 0xFFFF } else { delta as u16 };
    unsafe { (*ctx).last_report_time = current_time };

    // ── Build PTP report ─────────────────────────────────────────────
    let mut report = PtpReport::new();
    report.scan_time = scan_time;
    report.contact_count = count as u8;

    if unsafe { (*ctx).ptp_report_button.load(Ordering::Relaxed) } && button {
        report.is_button_clicked = 1;
    }

    if unsafe { (*ctx).ptp_report_touch.load(Ordering::Relaxed) } {
        for i in 0..count {
            let f = &fingers[i];
            let (x, y) = f.transform_to_ptp(config);
            report.contacts[i] = PtpContact::new(
                f.contact_id as u32,
                x,
                y,
                f.is_confident(),
                f.is_tip_switch(),
            );
        }
    }

    // ── Submit PTP report to VHF ─────────────────────────────────────
    if !unsafe { (*ctx).vhf_handle.is_null() }
        && unsafe { (*ctx).vhf_ready.load(Ordering::Relaxed) }
    {
        let report_bytes = report.as_bytes();
        let mut xfer_packet = HID_XFER_PACKET {
            reportBuffer: report_bytes.as_ptr() as *mut u8,
            reportBufferLen: report_bytes.len() as u32,
            reportId: report.report_id,
        };

        // Mark not ready until VHF signals via EvtVhfReadyForNextReadReport
        unsafe { (*ctx).vhf_ready.store(false, Ordering::Relaxed) };

        let status = unsafe {
            vhf_device::vhf_submit_read_report((*ctx).vhf_handle, &mut xfer_packet)
        };
        if !NT_SUCCESS(status) {
            // If submission failed, re-mark as ready to avoid stalling
            unsafe { (*ctx).vhf_ready.store(true, Ordering::Relaxed) };
        }
    }

    // ── Clean up and resubmit ────────────────────────────────────────
    unsafe {
        call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
    }

    // Issue next read request to keep the data flowing
    if unsafe { (*ctx).device_configured.load(Ordering::Relaxed) } {
        let _ = unsafe { transport::issue_read_request(device) };
    }
}
