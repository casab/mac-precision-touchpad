//! USB interrupt pipe continuous reader callback — touch data → PTP report.
//!
//! When the trackpad sends a USB interrupt transfer, WDF's continuous reader
//! invokes our callback. We parse the raw Apple finger data, build a Windows
//! PTP input report, and complete a pending HID read request.
//!
//! Ported from `Interrupt.c` (`AmtPtpEvtUsbInterruptPipeReadComplete`) in the C driver.

use wdk_sys::*;

use crate::device::get_device_context;

use amt_ptp_core::constants::PTP_MAX_CONTACT_POINTS;
use amt_ptp_core::finger::{parse_report, Finger};
use amt_ptp_core::ptp::{PtpContact, PtpReport};

/// Continuous reader completion callback for the USB interrupt pipe.
///
/// Called by WDF each time the trackpad delivers touch data. Steps:
/// 1. Validate the raw report
/// 2. Parse Apple finger data via [`parse_report`]
/// 3. Calculate scan time from performance counter delta
/// 4. Build a [`PtpReport`] with up to 5 contacts
/// 5. Dequeue a pending HID read request and complete it with the report
///
/// If no HID read request is pending, the touch data is silently discarded.
///
/// # Safety
///
/// Called by WDF with a valid pipe, memory buffer, and context (WDFDEVICE).
pub unsafe extern "C" fn evt_usb_interrupt_pipe_read_complete(
    _pipe: WDFUSBPIPE,
    buffer: WDFMEMORY,
    num_bytes_transferred: usize,
    context: WDFCONTEXT,
) {
    let device: WDFDEVICE = context.cast();
    let ctx = unsafe { &mut *get_device_context(device) };

    let config = match ctx.device_info {
        Some(c) => c,
        None => return,
    };

    // Skip if PTP input mode is not active
    if !ctx.ptp_input_on {
        return;
    }

    // Get raw buffer from WDF memory object
    let touch_buffer = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfMemoryGetBuffer,
            buffer,
            core::ptr::null_mut()
        )
    };
    if touch_buffer.is_null() || num_bytes_transferred == 0 {
        return;
    }

    let raw_data = unsafe {
        core::slice::from_raw_parts(touch_buffer as *const u8, num_bytes_transferred)
    };

    // ── Parse finger data ────────────────────────────────────────────
    let header_size = config.trackpad_type.header_size_usb();
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
        Err(_) => return, // malformed data — discard silently
    };

    // ── Calculate scan time ──────────────────────────────────────────
    //
    // Scan time is reported in 100µs units. We compute the delta between
    // the current performance counter and the last report's timestamp,
    // then divide by 100 to convert from ticks to 100µs units.
    let mut perf_counter: LARGE_INTEGER = unsafe { core::mem::zeroed() };
    unsafe {
        perf_counter = KeQueryPerformanceCounter(core::ptr::null_mut());
    }
    let current_time = unsafe { *perf_counter.QuadPart() };
    let delta = (current_time - ctx.last_report_time) / 100;
    let scan_time = if delta > 0xFFFF { 0xFFFF } else { delta as u16 };
    ctx.last_report_time = current_time;

    // ── Build PTP report ─────────────────────────────────────────────
    let mut report = PtpReport::new();
    report.scan_time = scan_time;
    report.contact_count = count as u8;

    if ctx.ptp_report_button && button {
        report.is_button_clicked = 1;
    }

    if ctx.ptp_report_touch {
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

    // ── Complete pending HID read request ─────────────────────────────
    let mut request: WDFREQUEST = core::ptr::null_mut();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfIoQueueRetrieveNextRequest,
            ctx.input_queue,
            &mut request
        )
    };
    if !NT_SUCCESS(status) {
        // No pending request — discard this report
        return;
    }

    let mut request_memory: WDFMEMORY = core::ptr::null_mut();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestRetrieveOutputMemory,
            request,
            &mut request_memory
        )
    };
    if !NT_SUCCESS(status) {
        unsafe {
            call_unsafe_wdf_function_binding!(WdfRequestComplete, request, status);
        }
        return;
    }

    let report_bytes = report.as_bytes();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfMemoryCopyFromBuffer,
            request_memory,
            0,
            report_bytes.as_ptr() as *const core::ffi::c_void,
            report_bytes.len()
        )
    };

    if NT_SUCCESS(status) {
        unsafe {
            call_unsafe_wdf_function_binding!(
                WdfRequestSetInformation,
                request,
                report_bytes.len() as u64
            );
        }
    }

    unsafe {
        call_unsafe_wdf_function_binding!(WdfRequestComplete, request, status);
    }
}
