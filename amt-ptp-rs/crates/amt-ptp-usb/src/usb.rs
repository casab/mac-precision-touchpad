//! USB pipe setup, Wellspring mode control, and continuous reader configuration.
//!
//! Ported from `Device.c` (SelectInterruptInterface, AmtPtpSetWellspringMode,
//! AmtPtpConfigContReaderForInterruptEndPoint) in the C driver.

use wdk::println;
use wdk_sys::*;

use crate::device::{get_device_context, DeviceContext};
use crate::input::evt_usb_interrupt_pipe_read_complete;
use amt_ptp_core::constants::*;
use amt_ptp_core::device::lookup_config;

/// Prepare USB hardware: create USB device, get descriptor, select interface, find pipes.
///
/// Called from `EvtDevicePrepareHardware`. This is where we discover the USB
/// device, look up its configuration in the device table, select the interface,
/// and configure the continuous reader.
///
/// # Safety
///
/// Called by WDF with a valid device handle. USB resources are allocated
/// and stored in the device context.
pub unsafe fn prepare_usb_hardware(device: WDFDEVICE) -> NTSTATUS {
    // SAFETY: device was created with DeviceContext
    let ctx = get_device_context(device);

    // 1. Create USB device handle (first time only)
    if unsafe { (*ctx).usb_device.is_null() } {
        let status = unsafe {
            call_unsafe_wdf_function_binding!(
                WdfUsbTargetDeviceCreate,
                device,
                WDF_NO_OBJECT_ATTRIBUTES,
                &mut (*ctx).usb_device
            )
        };
        if !NT_SUCCESS(status) {
            println!("WdfUsbTargetDeviceCreate failed: {status:#x}");
            return status;
        }
    }

    // 2. Get device descriptor (VID/PID)
    unsafe {
        call_unsafe_wdf_function_binding!(
            WdfUsbTargetDeviceGetDeviceDescriptor,
            (*ctx).usb_device,
            &mut (*ctx).device_descriptor
        );
    }

    // 3. Look up device config by product ID
    let product_id = unsafe { (*ctx).device_descriptor.idProduct };
    let config = lookup_config(product_id);
    unsafe { (*ctx).device_info = Some(config) };
    println!(
        "Apple trackpad detected: PID={product_id:#06x}, type={:?}",
        config.trackpad_type
    );

    // 4. Retrieve USB device information (traits)
    let mut device_info: WDF_USB_DEVICE_INFORMATION = unsafe { core::mem::zeroed() };
    device_info.Size = core::mem::size_of::<WDF_USB_DEVICE_INFORMATION>() as ULONG;
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfUsbTargetDeviceRetrieveInformation,
            (*ctx).usb_device,
            &mut device_info
        )
    };
    if NT_SUCCESS(status) {
        unsafe { (*ctx).usb_device_traits = device_info.Traits };
    }

    // 5. Select interface and find interrupt pipe
    let status = unsafe { select_interrupt_interface(device) };
    if !NT_SUCCESS(status) {
        println!("SelectInterruptInterface failed: {status:#x}");
        return status;
    }

    // 6. Configure continuous reader for the interrupt pipe
    let status = unsafe { configure_continuous_reader(device) };
    if !NT_SUCCESS(status) {
        println!("ConfigureContinuousReader failed: {status:#x}");
        return status;
    }

    STATUS_SUCCESS
}

/// Select the single USB interface and find the interrupt IN pipe.
///
/// # Safety
///
/// USB device must be created and stored in the device context.
unsafe fn select_interrupt_interface(device: WDFDEVICE) -> NTSTATUS {
    let ctx = get_device_context(device);

    // Select single interface configuration
    let mut config_params: WDF_USB_DEVICE_SELECT_CONFIG_PARAMS = unsafe { core::mem::zeroed() };
    // SAFETY: Initialize for single interface selection
    unsafe {
        WDF_USB_DEVICE_SELECT_CONFIG_PARAMS_INIT_SINGLE_INTERFACE(&mut config_params);
    }

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfUsbTargetDeviceSelectConfig,
            (*ctx).usb_device,
            WDF_NO_OBJECT_ATTRIBUTES,
            &mut config_params
        )
    };
    if !NT_SUCCESS(status) {
        return status;
    }

    // Extract configured interface and pipe count
    // SAFETY: After successful SelectConfig, the SingleInterface union member is valid
    let single_iface = unsafe { config_params.Types.SingleInterface };
    unsafe { (*ctx).usb_interface = single_iface.ConfiguredUsbInterface };
    let num_pipes = single_iface.NumberConfiguredPipes;

    // Find the interrupt IN pipe
    let mut found = false;
    for index in 0..num_pipes {
        let mut pipe_info: WDF_USB_PIPE_INFORMATION = unsafe { core::mem::zeroed() };
        pipe_info.Size = core::mem::size_of::<WDF_USB_PIPE_INFORMATION>() as ULONG;

        let pipe = unsafe {
            call_unsafe_wdf_function_binding!(
                WdfUsbInterfaceGetConfiguredPipe,
                (*ctx).usb_interface,
                index as u8,
                &mut pipe_info
            )
        };

        // Disable max packet size check (allow variable-length touch reports)
        unsafe {
            call_unsafe_wdf_function_binding!(
                WdfUsbTargetPipeSetNoMaximumPacketSizeCheck,
                pipe
            );
        }

        if pipe_info.PipeType == WDF_USB_PIPE_TYPE::WdfUsbPipeTypeInterrupt {
            unsafe { (*ctx).interrupt_pipe = pipe };
            found = true;
            break;
        }
    }

    if !found {
        println!("No interrupt pipe found!");
        return STATUS_DEVICE_CONFIGURATION_ERROR;
    }

    STATUS_SUCCESS
}

/// Configure the USB continuous reader for the interrupt pipe.
///
/// The continuous reader automatically submits read requests and calls
/// our callback when data arrives. Transfer length is computed from
/// the device type's header + MAX_FINGERS × finger_size.
///
/// # Safety
///
/// Interrupt pipe must be selected and stored in the device context.
unsafe fn configure_continuous_reader(device: WDFDEVICE) -> NTSTATUS {
    let ctx = get_device_context(device);
    let config = match unsafe { (*ctx).device_info } {
        Some(c) => c,
        None => return STATUS_DEVICE_NOT_READY,
    };

    let transfer_length = config.trackpad_type.usb_report_size();
    if transfer_length == 0 {
        return STATUS_UNKNOWN_REVISION;
    }

    let mut reader_config: WDF_USB_CONTINUOUS_READER_CONFIG = unsafe { core::mem::zeroed() };
    // SAFETY: Initialize the continuous reader config struct
    unsafe {
        WDF_USB_CONTINUOUS_READER_CONFIG_INIT(
            &mut reader_config,
            Some(evt_usb_interrupt_pipe_read_complete),
            device.cast(), // Context = WDFDEVICE (passed back in callback)
            transfer_length,
        );
    }
    reader_config.EvtUsbTargetPipeReadersFailed = Some(evt_usb_interrupt_readers_failed);

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfUsbTargetPipeConfigContinuousReader,
            (*ctx).interrupt_pipe,
            &mut reader_config
        )
    };

    status
}

/// Continuous reader failure callback.
///
/// Returns TRUE to have WDF reset the pipe and restart the reader.
pub unsafe extern "C" fn evt_usb_interrupt_readers_failed(
    _pipe: WDFUSBPIPE,
    _status: NTSTATUS,
    _usbd_status: USBD_STATUS,
) -> BOOLEAN {
    TRUE as BOOLEAN
}

/// Enable or disable Wellspring mode via USB control transfers.
///
/// Wellspring mode switches the trackpad from standard HID mouse reports
/// to raw multitouch data. Two control transfers: read current state,
/// modify the mode byte, write back.
///
/// # Safety
///
/// USB device must be ready and device_info must be set.
pub unsafe fn set_wellspring_mode(ctx: &mut DeviceContext, enable: bool) -> NTSTATUS {
    let config = match ctx.device_info {
        Some(c) => c,
        None => return STATUS_DEVICE_NOT_READY,
    };

    let msg = &config.wellspring_msg;
    let buf_size = msg.size as usize;

    // Allocate a buffer for the control transfer
    let mut buf_handle: WDFMEMORY = core::ptr::null_mut();
    let mut buffer: *mut core::ffi::c_void = core::ptr::null_mut();

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfMemoryCreate,
            WDF_NO_OBJECT_ATTRIBUTES,
            POOL_TYPE::PagedPool as u32,
            u32::from_le_bytes(*b"aptp"),  // pool tag
            buf_size,
            &mut buf_handle,
            &mut buffer
        )
    };
    if !NT_SUCCESS(status) {
        return status;
    }

    // 1. READ current mode
    let mut setup_packet: WDF_USB_CONTROL_SETUP_PACKET = unsafe { core::mem::zeroed() };
    setup_packet.Packet.bm.Bytes.Request = WELLSPRING_MODE_READ_REQUEST_ID;
    setup_packet.Packet.bm.Bytes.Direction = BMREQUEST_DEVICE_TO_HOST as u8;
    setup_packet.Packet.bm.Bytes.Recipient = BMREQUEST_TO_INTERFACE as u8;
    setup_packet.Packet.bm.Bytes.Type = BMREQUEST_CLASS as u8;
    setup_packet.Packet.wValue = msg.req_val;
    setup_packet.Packet.wIndex = msg.req_idx;

    let mut mem_desc: WDF_MEMORY_DESCRIPTOR = unsafe { core::mem::zeroed() };
    unsafe {
        WDF_MEMORY_DESCRIPTOR_INIT_BUFFER(
            &mut mem_desc,
            buffer,
            buf_size as ULONG,
        );
    }

    let mut bytes_transferred: ULONG = 0;
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfUsbTargetDeviceSendControlTransferSynchronously,
            ctx.usb_device,
            WDF_NO_HANDLE,
            core::ptr::null_mut(), // SendOptions
            &mut setup_packet,
            &mut mem_desc,
            &mut bytes_transferred
        )
    };
    if !NT_SUCCESS(status) {
        unsafe {
            call_unsafe_wdf_function_binding!(WdfObjectDelete, buf_handle.cast());
        }
        return status;
    }

    // 2. Modify the mode switch byte
    let buf_slice =
        unsafe { core::slice::from_raw_parts_mut(buffer.cast::<u8>(), buf_size) };
    let switch_idx = msg.switch_idx as usize;
    if switch_idx >= buf_size {
        println!("SetWellspringMode: switch_idx {switch_idx} >= buf_size {buf_size}");
        unsafe {
            call_unsafe_wdf_function_binding!(WdfObjectDelete, buf_handle.cast());
        }
        return STATUS_INVALID_PARAMETER;
    }
    buf_slice[switch_idx] = if enable { msg.switch_on } else { msg.switch_off };

    // 3. WRITE modified mode
    setup_packet.Packet.bm.Bytes.Request = WELLSPRING_MODE_WRITE_REQUEST_ID;
    setup_packet.Packet.bm.Bytes.Direction = BMREQUEST_HOST_TO_DEVICE as u8;

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfUsbTargetDeviceSendControlTransferSynchronously,
            ctx.usb_device,
            WDF_NO_HANDLE,
            core::ptr::null_mut(),
            &mut setup_packet,
            &mut mem_desc,
            &mut bytes_transferred
        )
    };

    // Cleanup
    unsafe {
        call_unsafe_wdf_function_binding!(WdfObjectDelete, buf_handle.cast());
    }

    if NT_SUCCESS(status) {
        ctx.is_wellspring_mode_on = enable;
    }

    status
}
