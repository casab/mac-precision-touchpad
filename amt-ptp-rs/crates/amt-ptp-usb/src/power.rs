//! Power management callbacks (D0Entry / D0Exit).
//!
//! Ported from `Device.c` (AmtPtpEvtDeviceD0Entry, AmtPtpEvtDeviceD0Exit).

use core::sync::atomic::Ordering;

use wdk::println;
use wdk_sys::*;

use crate::device::get_device_context;
use crate::usb::set_wellspring_mode;

/// EvtDeviceD0Entry — called when the device enters the D0 (working) power state.
///
/// Enables Wellspring mode and starts the USB continuous reader.
///
/// # Safety
///
/// Called by WDF with a valid device handle. The device must be fully
/// initialized (PrepareHardware completed).
pub unsafe extern "C" fn evt_device_d0_entry(
    device: WDFDEVICE,
    _previous_state: WDF_POWER_DEVICE_STATE,
) -> NTSTATUS {
    let ctx = get_device_context(device);

    // Enable Wellspring mode if reporting is requested
    if unsafe { (*ctx).ptp_report_button.load(Ordering::Relaxed) }
        || unsafe { (*ctx).ptp_report_touch.load(Ordering::Relaxed) }
    {
        let status = unsafe { set_wellspring_mode(&mut *ctx, true) };
        if !NT_SUCCESS(status) {
            println!("D0Entry: SetWellspringMode(ON) failed: {status:#x}");
            return status;
        }
    }

    // Record initial timestamp and frequency for scan time calculation.
    // The frequency is needed to convert tick deltas to 100µs PTP scan time units.
    // SAFETY: KeQueryPerformanceCounter is always safe to call.
    let mut freq: LARGE_INTEGER = unsafe { core::mem::zeroed() };
    let counter = unsafe { KeQueryPerformanceCounter(&mut freq) };
    unsafe {
        (*ctx).perf_freq = *freq.QuadPart();
        (*ctx).last_report_time = *counter.QuadPart();
    }

    // Start the continuous reader on the interrupt pipe
    if unsafe { (*ctx).interrupt_pipe.is_null() } {
        println!("D0Entry: interrupt_pipe is null");
        return STATUS_DEVICE_NOT_READY;
    }
    let io_target = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfUsbTargetPipeGetIoTarget,
            (*ctx).interrupt_pipe
        )
    };
    let status = unsafe {
        call_unsafe_wdf_function_binding!(WdfIoTargetStart, io_target)
    };
    if !NT_SUCCESS(status) {
        println!("D0Entry: WdfIoTargetStart failed: {status:#x}");
        // Stop the target if it was partially started
        unsafe {
            call_unsafe_wdf_function_binding!(
                WdfIoTargetStop,
                io_target,
                WDF_IO_TARGET_SENT_IO_ACTION::WdfIoTargetCancelSentIo
            );
        }
        return status;
    }

    STATUS_SUCCESS
}

/// EvtDeviceD0Exit — called when the device leaves the D0 power state.
///
/// Stops the continuous reader and disables Wellspring mode.
///
/// # Safety
///
/// Called by WDF with a valid device handle.
pub unsafe extern "C" fn evt_device_d0_exit(
    device: WDFDEVICE,
    _target_state: WDF_POWER_DEVICE_STATE,
) -> NTSTATUS {
    let ctx = get_device_context(device);

    // Stop the interrupt pipe I/O target
    if !unsafe { (*ctx).interrupt_pipe.is_null() } {
        let io_target = unsafe {
            call_unsafe_wdf_function_binding!(
                WdfUsbTargetPipeGetIoTarget,
                (*ctx).interrupt_pipe
            )
        };
        unsafe {
            call_unsafe_wdf_function_binding!(
                WdfIoTargetStop,
                io_target,
                WDF_IO_TARGET_SENT_IO_ACTION::WdfIoTargetCancelSentIo
            );
        }
    }

    // Disable Wellspring mode
    let status = unsafe { set_wellspring_mode(&mut *ctx, false) };
    if !NT_SUCCESS(status) {
        println!("D0Exit: SetWellspringMode(OFF) failed: {status:#x}");
        // Non-fatal — device is powering down anyway
    }

    STATUS_SUCCESS
}
