//! Bluetooth HID transport layer — I/O target, multitouch activation, read requests.
//!
//! The BT filter driver sits above the Bluetooth HID transport device. To read
//! raw touch data, we send `IOCTL_HID_READ_REPORT` requests to the default I/O
//! target (the lower device in the stack). To activate multitouch mode, we send
//! an `IOCTL_HID_SET_FEATURE` with report ID 0xF1.
//!
//! Unlike the USB driver's continuous reader (which auto-resubmits), we manually
//! create and send each read request, then resubmit from the completion callback.
//!
//! Ported from `Device.c` (`PtpFilterConfigureMultiTouch`) and `Input.c`
//! (`PtpFilterInputIssueTransportRequest`) in the C BT filter driver.

use wdk::println;
use wdk_sys::*;

use crate::device::{get_device_context, DeviceContext};
use crate::input::evt_bt_read_complete;

/// Buffer size for HID read requests (matches C driver's REPORT_BUFFER_SIZE).
const REPORT_BUFFER_SIZE: usize = 1024;

/// Pool tag for the lookaside list: 'apbt' (Apple Bluetooth).
const BT_POOL_TAG: u32 = u32::from_le_bytes(*b"apbt");

/// Initialize the BT HID transport: get I/O target and create lookaside list.
///
/// Called from `SelfManagedIoInit`. The default I/O target points to the lower
/// device in the filter stack (the BT HID transport driver).
///
/// # Safety
///
/// Device must be a valid WDFDEVICE with an initialized DeviceContext.
pub unsafe fn init_transport(device: WDFDEVICE) -> NTSTATUS {
    let ctx = unsafe { &mut *get_device_context(device) };

    // Get the default I/O target (lower device in filter stack)
    ctx.hid_io_target = unsafe {
        call_unsafe_wdf_function_binding!(WdfDeviceGetIoTarget, device)
    };
    if ctx.hid_io_target.is_null() {
        println!("transport: WdfDeviceGetIoTarget returned NULL");
        return STATUS_UNSUCCESSFUL;
    }

    // Create lookaside list for read request buffers
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfLookasideListCreate,
            WDF_NO_OBJECT_ATTRIBUTES,
            REPORT_BUFFER_SIZE,
            POOL_TYPE::NonPagedPoolNx as u32,
            WDF_NO_OBJECT_ATTRIBUTES,
            BT_POOL_TAG,
            &mut ctx.hid_read_buffer_lookaside
        )
    };
    if !NT_SUCCESS(status) {
        println!("transport: WdfLookasideListCreate failed: {status:#x}");
        return status;
    }

    STATUS_SUCCESS
}

/// Query VID/PID from the underlying HID device via IOCTL_HID_GET_DEVICE_ATTRIBUTES.
///
/// Sends a synchronous internal IOCTL to the lower device to retrieve the
/// HID_DEVICE_ATTRIBUTES (VID, PID, version).
///
/// # Safety
///
/// I/O target must be initialized (call after `init_transport`).
pub unsafe fn query_device_attributes(ctx: &mut DeviceContext) -> NTSTATUS {
    // HID_DEVICE_ATTRIBUTES from hidpi.h: Size + VendorID + ProductID + VersionNumber + Reserved[11]
    // Total = 4 + 2 + 2 + 2 + 22 = 32 bytes
    #[repr(C)]
    struct HidDeviceAttributes {
        size: u32,
        vendor_id: u16,
        product_id: u16,
        version_number: u16,
        reserved: [u16; 11],
    }

    let mut attrs: HidDeviceAttributes = unsafe { core::mem::zeroed() };
    attrs.size = core::mem::size_of::<HidDeviceAttributes>() as u32;

    let mut mem_desc: WDF_MEMORY_DESCRIPTOR = unsafe { core::mem::zeroed() };
    unsafe {
        WDF_MEMORY_DESCRIPTOR_INIT_BUFFER(
            &mut mem_desc,
            &mut attrs as *mut _ as *mut core::ffi::c_void,
            core::mem::size_of::<HidDeviceAttributes>() as ULONG,
        );
    }

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfIoTargetSendInternalIoctlSynchronously,
            ctx.hid_io_target,
            core::ptr::null_mut(), // Request (NULL = allocate one)
            IOCTL_HID_GET_DEVICE_ATTRIBUTES,
            core::ptr::null_mut(), // InputBuffer (none for GET)
            &mut mem_desc,         // OutputBuffer
            core::ptr::null_mut(), // RequestOptions
            core::ptr::null_mut()  // BytesReturned
        )
    };

    if NT_SUCCESS(status) {
        ctx.vendor_id = attrs.vendor_id;
        ctx.product_id = attrs.product_id;
        ctx.version_number = attrs.version_number;
        println!(
            "transport: device attributes VID={:#06x} PID={:#06x} ver={:#06x}",
            ctx.vendor_id, ctx.product_id, ctx.version_number
        );
    } else {
        println!("transport: IOCTL_HID_GET_DEVICE_ATTRIBUTES failed: {status:#x}");
    }

    status
}

/// Send the BT multitouch activation command (report 0xF1) to the trackpad.
///
/// This switches the Magic Trackpad 2 from standard HID mouse mode to raw
/// multitouch data mode. The BT activation packet is:
/// ```text
/// Report ID: 0xF1
/// Data: [0xF1, 0x02, 0x01]  (3 bytes)
/// ```
///
/// # Safety
///
/// I/O target must be initialized and device must be a Magic Trackpad 2.
pub unsafe fn activate_multitouch(ctx: &mut DeviceContext) -> NTSTATUS {
    // Build HID_XFER_PACKET for SET_FEATURE
    // The packet layout: HID_XFER_PACKET header followed by report data
    #[repr(C)]
    struct SetFeaturePacket {
        report_buffer: *mut u8,
        report_buffer_len: u32,
        report_id: u8,
    }

    // Report data: [0xF1, 0x02, 0x01]
    let mut report_data: [u8; 3] = [0xF1, 0x02, 0x01];

    // We need to build an IRP with UserBuffer = HID_XFER_PACKET.
    // Use WdfIoTargetSendInternalIoctlSynchronously with manual packet setup.

    // Create a request object
    let mut request: WDFREQUEST = core::ptr::null_mut();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestCreate,
            WDF_NO_OBJECT_ATTRIBUTES,
            ctx.hid_io_target,
            &mut request
        )
    };
    if !NT_SUCCESS(status) {
        println!("transport: WdfRequestCreate failed: {status:#x}");
        return status;
    }

    // Allocate memory for the HID_XFER_PACKET + report data buffer
    // HID_XFER_PACKET layout (from ntddk): reportBuffer (ptr), reportBufferLen (ULONG), reportId (UCHAR)
    // We'll use a stack-allocated buffer that holds the full packet struct
    // For IOCTL_HID_SET_FEATURE, the IRP's UserBuffer must point to HID_XFER_PACKET
    let mut xfer_packet: HID_XFER_PACKET = unsafe { core::mem::zeroed() };
    xfer_packet.reportBuffer = report_data.as_mut_ptr();
    xfer_packet.reportBufferLen = report_data.len() as u32;
    xfer_packet.reportId = 0xF1;

    // We need to allocate WDF memory for the input buffer
    let mut input_mem: WDFMEMORY = core::ptr::null_mut();
    let mut input_attrs: WDF_OBJECT_ATTRIBUTES = unsafe { core::mem::zeroed() };
    input_attrs.Size = core::mem::size_of::<WDF_OBJECT_ATTRIBUTES>() as ULONG;
    input_attrs.ParentObject = request.cast();

    let xfer_size = core::mem::size_of::<HID_XFER_PACKET>();
    let mut mem_buffer: *mut core::ffi::c_void = core::ptr::null_mut();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfMemoryCreate,
            &mut input_attrs,
            POOL_TYPE::NonPagedPoolNx as u32,
            BT_POOL_TAG,
            xfer_size,
            &mut input_mem,
            &mut mem_buffer
        )
    };
    if !NT_SUCCESS(status) {
        println!("transport: WdfMemoryCreate for xfer packet failed: {status:#x}");
        unsafe {
            call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
        }
        return status;
    }

    // Format request for internal IOCTL
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfIoTargetFormatRequestForInternalIoctl,
            ctx.hid_io_target,
            request,
            IOCTL_HID_SET_FEATURE,
            input_mem,
            core::ptr::null_mut(), // InputBufferOffset
            core::ptr::null_mut(), // OutputBuffer
            core::ptr::null_mut()  // OutputBufferOffset
        )
    };
    if !NT_SUCCESS(status) {
        println!("transport: WdfIoTargetFormatRequestForInternalIoctl failed: {status:#x}");
        unsafe {
            call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
        }
        return status;
    }

    // Critical: Set the IRP's UserBuffer to point to our HID_XFER_PACKET
    // This is required because HID minidrivers read HID_XFER_PACKET from UserBuffer
    let irp = unsafe {
        call_unsafe_wdf_function_binding!(WdfRequestWdmGetIrp, request)
    };
    if !irp.is_null() {
        unsafe { (*irp).UserBuffer = &mut xfer_packet as *mut _ as *mut core::ffi::c_void };
    }

    // Send synchronously
    let mut send_options: WDF_REQUEST_SEND_OPTIONS = unsafe { core::mem::zeroed() };
    send_options.Size = core::mem::size_of::<WDF_REQUEST_SEND_OPTIONS>() as ULONG;
    send_options.Flags = WDF_REQUEST_SEND_OPTIONS_FLAGS::WDF_REQUEST_SEND_OPTION_SYNCHRONOUS as u32;

    let sent = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestSend,
            request,
            ctx.hid_io_target,
            &mut send_options
        )
    };

    let result = if sent == 0 {
        let status = unsafe {
            call_unsafe_wdf_function_binding!(WdfRequestGetStatus, request)
        };
        println!("transport: multitouch activation failed: {status:#x}");
        status
    } else {
        println!("transport: multitouch mode activated (0xF1)");
        STATUS_SUCCESS
    };

    // Clean up the request
    unsafe {
        call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
    }

    result
}

/// Issue an asynchronous HID read request to the BT transport.
///
/// Creates a new WDF request, allocates a buffer from the lookaside list,
/// formats it as `IOCTL_HID_READ_REPORT`, and sends it to the lower device.
/// When data arrives, [`evt_bt_read_complete`] is called.
///
/// # Safety
///
/// I/O target and lookaside list must be initialized. Device must be configured.
pub unsafe fn issue_read_request(device: WDFDEVICE) -> NTSTATUS {
    let ctx = unsafe { &*get_device_context(device) };

    if !ctx.device_configured || ctx.hid_io_target.is_null() {
        return STATUS_DEVICE_NOT_READY;
    }

    // Create a request parented to the device
    let mut attrs: WDF_OBJECT_ATTRIBUTES = unsafe { core::mem::zeroed() };
    attrs.Size = core::mem::size_of::<WDF_OBJECT_ATTRIBUTES>() as ULONG;
    attrs.ParentObject = device.cast();

    let mut request: WDFREQUEST = core::ptr::null_mut();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestCreate,
            &mut attrs,
            ctx.hid_io_target,
            &mut request
        )
    };
    if !NT_SUCCESS(status) {
        println!("transport: WdfRequestCreate for read failed: {status:#x}");
        return status;
    }

    // Allocate output buffer from lookaside list
    let mut output_mem: WDFMEMORY = core::ptr::null_mut();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfMemoryCreateFromLookaside,
            ctx.hid_read_buffer_lookaside,
            &mut output_mem
        )
    };
    if !NT_SUCCESS(status) {
        println!("transport: WdfMemoryCreateFromLookaside failed: {status:#x}");
        unsafe {
            call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
        }
        return status;
    }

    // Format request for internal IOCTL read
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfIoTargetFormatRequestForInternalIoctl,
            ctx.hid_io_target,
            request,
            IOCTL_HID_READ_REPORT,
            core::ptr::null_mut(), // InputBuffer (none for read)
            core::ptr::null_mut(), // InputBufferOffset
            output_mem,            // OutputBuffer
            core::ptr::null_mut()  // OutputBufferOffset
        )
    };
    if !NT_SUCCESS(status) {
        println!("transport: format read request failed: {status:#x}");
        unsafe {
            call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
        }
        return status;
    }

    // Set completion callback — passes the WDFDEVICE as context
    unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestSetCompletionRoutine,
            request,
            Some(evt_bt_read_complete),
            device as WDFCONTEXT
        );
    }

    // Send asynchronously (no send options = async)
    let sent = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfRequestSend,
            request,
            ctx.hid_io_target,
            core::ptr::null_mut() // NULL = default (async)
        )
    };

    if sent == 0 {
        let status = unsafe {
            call_unsafe_wdf_function_binding!(WdfRequestGetStatus, request)
        };
        println!("transport: WdfRequestSend for read failed: {status:#x}");
        unsafe {
            call_unsafe_wdf_function_binding!(WdfObjectDelete, request.cast());
        }
        return status;
    }

    STATUS_SUCCESS
}
