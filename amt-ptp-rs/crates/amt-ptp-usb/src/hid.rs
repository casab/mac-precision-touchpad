//! HID descriptor, device attributes, report descriptor, and feature report handlers.
//!
//! Serves HID data to the upper-layer HID class driver (hidclass.sys) in
//! response to `IOCTL_HID_*` internal device control requests.
//!
//! Ported from `Hid.c` in the C driver.

use core::sync::atomic::Ordering;

use wdk::println;
use wdk_sys::*;

use crate::device::get_device_context;
use crate::usb::set_wellspring_mode;

use amt_ptp_core::constants::*;
use amt_ptp_core::hid::{build_report_descriptor, DEFAULT_HQA_BLOB};
use amt_ptp_core::ptp::*;

// ── HID Types ───────────────────────────────────────────────────────
//
// These mirror the Windows HID structs from hid.h / hidport.h.
// We define them here because wdk-sys may not expose them directly.

/// HID descriptor (9 bytes). Matches `HID_DESCRIPTOR` from hid.h.
#[repr(C, packed)]
struct HidDescriptor {
    b_length: u8,
    b_descriptor_type: u8,
    bcd_hid: u16,
    b_country_code: u8,
    b_num_descriptors: u8,
    // Descriptor list entry (only one for us)
    b_report_type: u8,
    w_report_length: u16,
}

const HID_DESCRIPTOR_SIZE: usize = core::mem::size_of::<HidDescriptor>();

/// HID device attributes. Matches `HID_DEVICE_ATTRIBUTES` from hidclass.h.
#[repr(C)]
struct HidDeviceAttributes {
    size: ULONG,
    vendor_id: u16,
    product_id: u16,
    version_number: u16,
    reserved: [u16; 11],
}

const HID_DEVICE_ATTRIBUTES_SIZE: usize = core::mem::size_of::<HidDeviceAttributes>();

/// HID transfer packet for GET/SET feature. Matches `HID_XFER_PACKET`.
#[repr(C)]
struct HidXferPacket {
    report_buffer: *mut u8,
    report_buffer_len: ULONG,
    report_id: u8,
}

// ── Descriptor Serving ──────────────────────────────────────────────

/// Handle `IOCTL_HID_GET_DEVICE_DESCRIPTOR`.
///
/// Returns the 9-byte HID descriptor containing the report descriptor length.
///
/// # Safety
///
/// Request must be valid with a sufficiently large output buffer.
pub unsafe fn get_hid_descriptor(device: WDFDEVICE, request: WDFREQUEST) -> NTSTATUS {
    // SAFETY: device was created with DeviceContext
    let ctx = unsafe { get_device_context(device) };
    let config = match unsafe { (*ctx).device_info } {
        Some(c) => c,
        None => return STATUS_DEVICE_NOT_READY,
    };

    // Build report descriptor to determine its length
    let report_desc = build_report_descriptor(
        config.ptp_x_logical_max(),
        config.ptp_y_logical_max(),
        config.ptp_x_physical_max(),
        config.ptp_y_physical_max(),
    );

    let descriptor = HidDescriptor {
        b_length: HID_DESCRIPTOR_SIZE as u8,
        b_descriptor_type: 0x21, // HID
        bcd_hid: 0x0100,
        b_country_code: 0x00,
        b_num_descriptors: 0x01,
        b_report_type: 0x22, // Report
        w_report_length: report_desc.len() as u16,
    };

    let mut memory: WDFMEMORY = core::ptr::null_mut();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestRetrieveOutputMemory,
            request,
            &mut memory
        )
    };
    if !NT_SUCCESS(status) {
        return status;
    }

    let copy_size = HID_DESCRIPTOR_SIZE;
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfMemoryCopyFromBuffer,
            memory,
            0,
            &descriptor as *const HidDescriptor as *const core::ffi::c_void,
            copy_size
        )
    };
    if !NT_SUCCESS(status) {
        return status;
    }

    unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestSetInformation,
            request,
            copy_size as u64
        );
    }

    STATUS_SUCCESS
}

/// Handle `IOCTL_HID_GET_DEVICE_ATTRIBUTES`.
///
/// Returns VID, PID, and version from the USB device descriptor.
///
/// # Safety
///
/// Request must be valid with a sufficiently large output buffer.
pub unsafe fn get_device_attributes(device: WDFDEVICE, request: WDFREQUEST) -> NTSTATUS {
    // SAFETY: device was created with DeviceContext
    let ctx = unsafe { get_device_context(device) };

    let mut buffer: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut buffer_len: usize = 0;

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestRetrieveOutputBuffer,
            request,
            HID_DEVICE_ATTRIBUTES_SIZE,
            &mut buffer,
            &mut buffer_len
        )
    };
    if !NT_SUCCESS(status) {
        return status;
    }

    let attrs = unsafe { &mut *(buffer as *mut HidDeviceAttributes) };
    attrs.size = HID_DEVICE_ATTRIBUTES_SIZE as ULONG;
    attrs.vendor_id = unsafe { (*ctx).device_descriptor.idVendor };
    attrs.product_id = unsafe { (*ctx).device_descriptor.idProduct };
    attrs.version_number = DEVICE_VERSION as u16;
    attrs.reserved = [0u16; 11];

    unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestSetInformation,
            request,
            HID_DEVICE_ATTRIBUTES_SIZE as u64
        );
    }

    STATUS_SUCCESS
}

/// Handle `IOCTL_HID_GET_REPORT_DESCRIPTOR`.
///
/// Returns the full HID report descriptor, dynamically built from the
/// device config's coordinate ranges.
///
/// # Safety
///
/// Request must be valid with a sufficiently large output buffer.
pub unsafe fn get_report_descriptor(device: WDFDEVICE, request: WDFREQUEST) -> NTSTATUS {
    // SAFETY: device was created with DeviceContext
    let ctx = unsafe { get_device_context(device) };
    let config = match unsafe { (*ctx).device_info } {
        Some(c) => c,
        None => return STATUS_DEVICE_NOT_READY,
    };

    let report_desc = build_report_descriptor(
        config.ptp_x_logical_max(),
        config.ptp_y_logical_max(),
        config.ptp_x_physical_max(),
        config.ptp_y_physical_max(),
    );

    let mut memory: WDFMEMORY = core::ptr::null_mut();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestRetrieveOutputMemory,
            request,
            &mut memory
        )
    };
    if !NT_SUCCESS(status) {
        return status;
    }

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfMemoryCopyFromBuffer,
            memory,
            0,
            report_desc.as_ptr() as *const core::ffi::c_void,
            report_desc.len()
        )
    };
    if !NT_SUCCESS(status) {
        return status;
    }

    unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestSetInformation,
            request,
            report_desc.len() as u64
        );
    }

    STATUS_SUCCESS
}

// ── Feature Report Handlers ─────────────────────────────────────────

/// Handle `IOCTL_HID_GET_FEATURE`.
///
/// Fills the caller's buffer with the requested feature report data:
/// - **Report 0x07** (Device Caps): max contacts = 5, button type = clickpad
/// - **Report 0x08** (HQA): 256-byte certification blob
///
/// # Safety
///
/// Request IRP `UserBuffer` must contain a valid `HID_XFER_PACKET`.
pub unsafe fn get_feature(_device: WDFDEVICE, request: WDFREQUEST) -> NTSTATUS {
    let irp = unsafe {
        call_unsafe_wdf_function_binding!(WdfRequestWdmGetIrp, request)
    };
    if irp.is_null() {
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    let packet = unsafe { (*irp).UserBuffer as *mut HidXferPacket };
    if packet.is_null() {
        return STATUS_INVALID_DEVICE_REQUEST;
    }
    let packet = unsafe { &mut *packet };

    match packet.report_id {
        REPORTID_DEVICE_CAPS => {
            let report_size = core::mem::size_of::<PtpDeviceCapsReport>();
            if (packet.report_buffer_len as usize) < report_size {
                return STATUS_BUFFER_TOO_SMALL;
            }

            let report = PtpDeviceCapsReport::default_clickpad();
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &report as *const PtpDeviceCapsReport as *const u8,
                    packet.report_buffer,
                    report_size,
                );
            }

            unsafe {
                call_unsafe_wdf_function_binding!(
                    WdfRequestSetInformation,
                    request,
                    report_size as u64
                );
            }
            STATUS_SUCCESS
        }

        REPORTID_PTPHQA => {
            let report_size = core::mem::size_of::<PtpHqaCertificationReport>();
            if (packet.report_buffer_len as usize) < report_size {
                return STATUS_BUFFER_TOO_SMALL;
            }

            let report = PtpHqaCertificationReport {
                report_id: REPORTID_PTPHQA,
                blob: DEFAULT_HQA_BLOB,
            };
            unsafe {
                core::ptr::copy_nonoverlapping(
                    &report as *const PtpHqaCertificationReport as *const u8,
                    packet.report_buffer,
                    report_size,
                );
            }

            unsafe {
                call_unsafe_wdf_function_binding!(
                    WdfRequestSetInformation,
                    request,
                    report_size as u64
                );
            }
            STATUS_SUCCESS
        }

        other => {
            println!("GetFeature: unsupported report ID {other:#x}");
            STATUS_NOT_SUPPORTED
        }
    }
}

/// Handle `IOCTL_HID_SET_FEATURE`.
///
/// Processes incoming feature report writes:
/// - **Report 0x04** (Input Mode): switches to PTP mode, enables Wellspring
/// - **Report 0x06** (Selective Reporting): enables/disables button + surface
///
/// # Safety
///
/// Request IRP `UserBuffer` must contain a valid `HID_XFER_PACKET`.
pub unsafe fn set_feature(device: WDFDEVICE, request: WDFREQUEST) -> NTSTATUS {
    // SAFETY: device was created with DeviceContext
    let ctx = unsafe { get_device_context(device) };

    let irp = unsafe {
        call_unsafe_wdf_function_binding!(WdfRequestWdmGetIrp, request)
    };
    if irp.is_null() {
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    let packet = unsafe { (*irp).UserBuffer as *const HidXferPacket };
    if packet.is_null() {
        return STATUS_INVALID_DEVICE_REQUEST;
    }
    let packet = unsafe { &*packet };

    match packet.report_id {
        REPORTID_INPUT_MODE => {
            if (packet.report_buffer_len as usize) < core::mem::size_of::<PtpInputModeReport>() {
                return STATUS_BUFFER_TOO_SMALL;
            }

            let report = unsafe { &*(packet.report_buffer as *const PtpInputModeReport) };
            let mode = { report.mode }; // copy from packed field
            if mode == PTP_COLLECTION_WINDOWS {
                let status = unsafe { set_wellspring_mode(&mut *ctx, true) };
                if !NT_SUCCESS(status) {
                    println!("SetFeature: Wellspring enable failed: {status:#x}");
                    return status;
                }
                unsafe { (*ctx).ptp_input_on.store(true, Ordering::Relaxed) };
            } else {
                unsafe { (*ctx).ptp_input_on.store(false, Ordering::Relaxed) };
            }
            STATUS_SUCCESS
        }

        REPORTID_FUNC_SWITCH => {
            if (packet.report_buffer_len as usize)
                < core::mem::size_of::<PtpSelectiveReportingReport>()
            {
                return STATUS_BUFFER_TOO_SMALL;
            }

            let report =
                unsafe { &*(packet.report_buffer as *const PtpSelectiveReportingReport) };
            let button = report.button_report_on();
            let surface = report.surface_report_on();
            unsafe {
                (*ctx).ptp_report_button.store(button, Ordering::Relaxed);
                (*ctx).ptp_report_touch.store(surface, Ordering::Relaxed);
            }
            println!(
                "SelectiveReporting: button={button}, surface={surface}"
            );
            STATUS_SUCCESS
        }

        other => {
            println!("SetFeature: unsupported report ID {other:#x}");
            STATUS_NOT_SUPPORTED
        }
    }
}
