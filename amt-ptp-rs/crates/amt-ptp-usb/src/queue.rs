//! Queue initialization and HID IOCTL dispatch.
//!
//! Two I/O queues:
//! 1. **Default parallel queue** — routes `IOCTL_HID_*` internal device control
//!    requests to the appropriate handler.
//! 2. **Manual input queue** — holds pending `IOCTL_HID_READ_REPORT` requests
//!    until the continuous reader callback has data to deliver.
//!
//! Ported from `Queue.c` in the C driver.

use wdk::println;
use wdk_sys::*;

use crate::device::get_device_context;
use crate::hid;

// ── HID Internal IOCTL Codes ────────────────────────────────────────
//
// From hidport.h:
//   CTL_CODE(FILE_DEVICE_KEYBOARD, id, METHOD_NEITHER, FILE_ANY_ACCESS)
//
// FILE_DEVICE_KEYBOARD = 0x0B, METHOD_NEITHER = 3, FILE_ANY_ACCESS = 0
const fn hid_ctl_code(id: u32) -> ULONG {
    (0x0Bu32 << 16) | (id << 2) | 3
}

const IOCTL_HID_GET_DEVICE_DESCRIPTOR: ULONG = hid_ctl_code(0);
const IOCTL_HID_GET_REPORT_DESCRIPTOR: ULONG = hid_ctl_code(1);
const IOCTL_HID_READ_REPORT: ULONG = hid_ctl_code(2);
const IOCTL_HID_GET_DEVICE_ATTRIBUTES: ULONG = hid_ctl_code(9);
const IOCTL_HID_GET_FEATURE: ULONG = hid_ctl_code(101);
const IOCTL_HID_SET_FEATURE: ULONG = hid_ctl_code(100);

/// Initialize I/O queues for the device.
///
/// Creates:
/// - Default parallel queue with `EvtIoInternalDeviceControl` for HID IOCTLs
/// - Manual (non-power-managed) queue for pending HID read report requests
///
/// # Safety
///
/// Device must be created with a [`DeviceContext`](crate::device::DeviceContext).
pub unsafe fn queue_initialize(device: WDFDEVICE) -> NTSTATUS {
    // SAFETY: device was created with DeviceContext
    let ctx = unsafe { get_device_context(device) };

    // 1. Default parallel queue for HID IOCTLs
    let mut queue_config: WDF_IO_QUEUE_CONFIG = unsafe { core::mem::zeroed() };
    queue_config.Size = core::mem::size_of::<WDF_IO_QUEUE_CONFIG>() as ULONG;
    queue_config.DefaultQueue = TRUE as BOOLEAN;
    queue_config.DispatchType = WdfIoQueueDispatchParallel;
    queue_config.PowerManaged = WdfUseDefault;
    queue_config.EvtIoInternalDeviceControl = Some(evt_io_internal_device_control);
    queue_config.EvtIoStop = Some(evt_io_stop);

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfIoQueueCreate,
            device,
            &mut queue_config,
            WDF_NO_OBJECT_ATTRIBUTES,
            core::ptr::null_mut() // don't need the queue handle
        )
    };
    if !NT_SUCCESS(status) {
        println!("Default queue creation failed: {status:#x}");
        return status;
    }

    // 2. Manual queue for pending read report requests (not power-managed,
    //    so requests survive D0Exit → D0Entry transitions)
    let mut input_queue_config: WDF_IO_QUEUE_CONFIG = unsafe { core::mem::zeroed() };
    input_queue_config.Size = core::mem::size_of::<WDF_IO_QUEUE_CONFIG>() as ULONG;
    input_queue_config.DispatchType = WdfIoQueueDispatchManual;
    input_queue_config.PowerManaged = WdfFalse;

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfIoQueueCreate,
            device,
            &mut input_queue_config,
            WDF_NO_OBJECT_ATTRIBUTES,
            &mut (*ctx).input_queue
        )
    };
    if !NT_SUCCESS(status) {
        println!("Input queue creation failed: {status:#x}");
        return status;
    }

    STATUS_SUCCESS
}

/// EvtIoStop callback for the default parallel queue.
///
/// Called by WDF when the queue is being stopped (device leaving D0).
/// For a HID miniport's parallel queue, all requests complete quickly
/// (no long-running I/O), so we simply acknowledge the stop.
///
/// Without this callback, WDF Verifier warns that a power-managed queue
/// with pending requests has no EvtIoStop handler.
///
/// # Safety
///
/// Called by WDF with a valid queue, request, and action flags.
unsafe extern "C" fn evt_io_stop(
    _queue: WDFQUEUE,
    _request: WDFREQUEST,
    _action_flags: ULONG,
) {
    // No action needed — HID IOCTL requests on this queue complete
    // synchronously within the EvtIoInternalDeviceControl callback.
    // WDF will cancel/complete them automatically during power transitions.
}

/// Internal device control dispatch for HID miniport IOCTLs.
///
/// Routes each `IOCTL_HID_*` to the appropriate handler. Requests that are
/// forwarded to another queue (READ_REPORT) are marked pending and not
/// completed here.
///
/// # Safety
///
/// Called by WDF for each internal IOCTL arriving on the default queue.
unsafe extern "C" fn evt_io_internal_device_control(
    queue: WDFQUEUE,
    request: WDFREQUEST,
    _output_buffer_length: usize,
    _input_buffer_length: usize,
    io_control_code: ULONG,
) {
    let device = unsafe {
        call_unsafe_wdf_function_binding!(WdfIoQueueGetDevice, queue)
    };

    let mut pending = false;
    let status = match io_control_code {
        IOCTL_HID_GET_DEVICE_DESCRIPTOR => unsafe {
            hid::get_hid_descriptor(device, request)
        },
        IOCTL_HID_GET_DEVICE_ATTRIBUTES => unsafe {
            hid::get_device_attributes(device, request)
        },
        IOCTL_HID_GET_REPORT_DESCRIPTOR => unsafe {
            hid::get_report_descriptor(device, request)
        },
        IOCTL_HID_READ_REPORT => unsafe {
            dispatch_read_report(device, request, &mut pending)
        },
        IOCTL_HID_GET_FEATURE => unsafe {
            hid::get_feature(device, request)
        },
        IOCTL_HID_SET_FEATURE => unsafe {
            hid::set_feature(device, request)
        },
        _ => STATUS_NOT_SUPPORTED,
    };

    if !pending {
        unsafe {
            call_unsafe_wdf_function_binding!(WdfRequestComplete, request, status);
        }
    }
}

/// Forward a HID read report request to the manual input queue.
///
/// The continuous reader callback ([`crate::input::evt_usb_interrupt_pipe_read_complete`])
/// dequeues and completes these when touch data arrives.
unsafe fn dispatch_read_report(
    device: WDFDEVICE,
    request: WDFREQUEST,
    pending: &mut bool,
) -> NTSTATUS {
    // SAFETY: device was created with DeviceContext
    let ctx = unsafe { get_device_context(device) };

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestForwardToIoQueue,
            request,
            (*ctx).input_queue
        )
    };

    if NT_SUCCESS(status) {
        *pending = true;
    }

    status
}
