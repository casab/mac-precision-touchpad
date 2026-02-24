//! Recovery timer for automatic retry on transport failures.
//!
//! The BT trackpad may not respond to the multitouch activation command
//! immediately (e.g., still initializing after power-on), or a read request
//! may fail due to transient transport issues. The recovery mechanism
//! handles this:
//!
//! 1. On failure, a 2-second timer is started
//! 2. Timer fires at PASSIVE_LEVEL → re-activates multitouch → reissues reads
//! 3. If recovery also fails, the timer restarts for another attempt
//!
//! This matches the C driver's `PtpFilterRecoveryTimerCallback` pattern
//! from `Device.c`.

use core::sync::atomic::Ordering;

use wdk::println;
use wdk_sys::ntddk::KeQueryPerformanceCounter;
use wdk_sys::*;

use crate::device::get_device_context;
use crate::transport;

/// Recovery retry interval in seconds.
const RECOVERY_INTERVAL_SEC: i64 = 2;

/// Maximum number of consecutive recovery attempts before giving up.
const MAX_RECOVERY_ATTEMPTS: u32 = 10;

/// Create the recovery timer during device initialization.
///
/// The timer is configured to fire at PASSIVE_LEVEL with automatic
/// serialization, parented to the device object.
///
/// # Safety
///
/// Device must be a valid WDFDEVICE with an initialized DeviceContext.
pub unsafe fn create_recovery_objects(device: WDFDEVICE) -> NTSTATUS {
    // SAFETY: device was created with DeviceContext
    let ctx = unsafe { get_device_context(device) };

    // ── Recovery Timer ───────────────────────────────────────────
    let mut timer_config: WDF_TIMER_CONFIG = unsafe { core::mem::zeroed() };
    timer_config.Size = core::mem::size_of::<WDF_TIMER_CONFIG>() as ULONG;
    timer_config.EvtTimerFunc = Some(evt_recovery_timer);
    timer_config.AutomaticSerialization = TRUE as BOOLEAN;

    let mut timer_attrs: WDF_OBJECT_ATTRIBUTES = unsafe { core::mem::zeroed() };
    timer_attrs.Size = core::mem::size_of::<WDF_OBJECT_ATTRIBUTES>() as ULONG;
    timer_attrs.ParentObject = device.cast();
    timer_attrs.ExecutionLevel = WdfExecutionLevelPassive;

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfTimerCreate,
            &mut timer_config,
            &mut timer_attrs,
            &mut (*ctx).recovery_timer
        )
    };
    if !NT_SUCCESS(status) {
        println!("recovery: WdfTimerCreate failed: {status:#x}");
        return status;
    }

    STATUS_SUCCESS
}

/// Start the recovery timer.
///
/// Schedules the recovery timer to fire after [`RECOVERY_INTERVAL_SEC`] seconds.
/// Safe to call multiple times — WDF restarts the timer from the new due time.
///
/// # Safety
///
/// The recovery timer must have been created via [`create_recovery_objects`].
pub unsafe fn start_recovery_timer(ctx: &crate::device::DeviceContext) {
    if !ctx.recovery_timer.is_null() {
        // WDF_REL_TIMEOUT_IN_SEC: negative value in 100ns units
        let due_time = -RECOVERY_INTERVAL_SEC * 10_000_000;
        unsafe {
            call_unsafe_wdf_function_binding!(
                WdfTimerStart,
                ctx.recovery_timer,
                due_time
            );
        }
    }
}

/// Stop the recovery timer.
///
/// Cancels any pending timer. Returns TRUE if the timer was in the queue.
///
/// # Safety
///
/// The recovery timer must have been created via [`create_recovery_objects`].
pub unsafe fn stop_recovery_timer(ctx: &crate::device::DeviceContext) {
    if !ctx.recovery_timer.is_null() {
        unsafe {
            call_unsafe_wdf_function_binding!(
                WdfTimerStop,
                ctx.recovery_timer,
                TRUE as BOOLEAN // Wait for timer callback to complete
            );
        }
    }
}

/// Recovery timer callback.
///
/// Fires at PASSIVE_LEVEL after a failed multitouch activation or read request.
/// Attempts to re-activate multitouch mode and reissue read requests.
/// If recovery fails, restarts the timer for another attempt (up to
/// [`MAX_RECOVERY_ATTEMPTS`] times).
///
/// # Safety
///
/// Called by WDF with a valid timer handle parented to the device.
unsafe extern "C" fn evt_recovery_timer(timer: WDFTIMER) {
    let device: WDFDEVICE = unsafe {
        call_unsafe_wdf_function_binding!(WdfTimerGetParentObject, timer).cast()
    };
    // SAFETY: device was created with DeviceContext
    let ctx = unsafe { get_device_context(device) };

    // Don't recover if device is shutting down or suspended
    if unsafe { (*ctx).hid_io_target.is_null() }
        || !unsafe { (*ctx).device_configured.load(Ordering::Relaxed) }
    {
        return;
    }

    unsafe { (*ctx).recovery_attempts += 1 };
    println!(
        "recovery: attempt {} of {MAX_RECOVERY_ATTEMPTS}",
        unsafe { (*ctx).recovery_attempts }
    );

    // Try to activate multitouch mode
    let status = unsafe { transport::activate_multitouch(&mut *ctx) };
    if !NT_SUCCESS(status) {
        println!("recovery: multitouch activation failed: {status:#x}");
        if unsafe { (*ctx).recovery_attempts } < MAX_RECOVERY_ATTEMPTS {
            unsafe { start_recovery_timer(&*ctx) };
        } else {
            println!("recovery: max attempts reached, giving up");
        }
        return;
    }

    // Multitouch activated — mark device as configured and issue read request
    unsafe { (*ctx).device_configured.store(true, Ordering::Relaxed) };

    // Record fresh timestamp
    let mut freq: LARGE_INTEGER = unsafe { core::mem::zeroed() };
    let counter = unsafe { KeQueryPerformanceCounter(&mut freq) };
    unsafe {
        (*ctx).perf_freq = freq.QuadPart;
        (*ctx).last_report_time = counter.QuadPart;
    }

    let status = unsafe { transport::issue_read_request(device) };
    if NT_SUCCESS(status) {
        unsafe { (*ctx).recovery_attempts = 0 };
        println!("recovery: device recovered successfully");
    } else {
        println!("recovery: read request failed: {status:#x}");
        if unsafe { (*ctx).recovery_attempts } < MAX_RECOVERY_ATTEMPTS {
            unsafe { start_recovery_timer(&*ctx) };
        }
    }
}
