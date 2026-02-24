//! # amt-ptp-usb
//!
//! KMDF USB lower-filter driver for Apple Magic Trackpad 2/3 and T2 trackpads.
//!
//! This driver sits below `mshidkmdf` in the device stack and presents a
//! Windows Precision Touchpad (PTP) to the HID class driver:
//!
//! - Selects the correct USB interface and interrupt pipe
//! - Enables Wellspring mode (raw multitouch data) via USB control transfers
//! - Reads touch data from the USB interrupt pipe (continuous reader)
//! - Transforms Apple TYPE5/TYPE4 finger data into Windows PTP reports
//! - Serves HID descriptors, device attributes, and feature reports
//!
//! ## Supported Devices
//! - Apple Magic Trackpad 2 (USB, PID 0x0265)
//! - Apple T2 chip trackpads (MacBook Pro/Air 2018-2020)

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

#[cfg(not(test))]
extern crate wdk_panic;

#[cfg(not(test))]
use wdk_alloc::WdkAllocator;

use wdk::println;
use wdk_sys::*;

#[cfg(not(test))]
#[global_allocator]
static GLOBAL_ALLOCATOR: WdkAllocator = WdkAllocator;

mod device;
mod hid;
mod input;
mod power;
mod queue;
mod usb;

use device::{get_device_context, DEVICE_CONTEXT_TYPE_INFO};
use power::{evt_device_d0_entry, evt_device_d0_exit};
use queue::queue_initialize;
use usb::prepare_usb_hardware;

/// Driver entry point.
///
/// Initializes WDF, registers `EvtDriverDeviceAdd`, and returns.
///
/// # Safety
///
/// Called by the Windows kernel with valid pointers to the driver object and
/// registry path. Must not be called from user code.
// SAFETY: "DriverEntry" is the required symbol name for Windows driver entry points.
#[export_name = "DriverEntry"]
pub unsafe extern "system" fn driver_entry(
    driver: PDRIVER_OBJECT,
    registry_path: PCUNICODE_STRING,
) -> NTSTATUS {
    // Initialize WDF driver config
    let mut driver_config: WDF_DRIVER_CONFIG = unsafe { core::mem::zeroed() };
    driver_config.Size = core::mem::size_of::<WDF_DRIVER_CONFIG>() as ULONG;
    driver_config.EvtDriverDeviceAdd = Some(evt_driver_device_add);

    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfDriverCreate,
            driver,
            registry_path,
            WDF_NO_OBJECT_ATTRIBUTES,
            &mut driver_config,
            core::ptr::null_mut() // don't need driver handle
        )
    };

    if !NT_SUCCESS(status) {
        println!("WdfDriverCreate failed: {status:#x}");
    }

    status
}

/// EvtDriverDeviceAdd — called by WDF for each matching PnP device.
///
/// Sets up the device as a filter, registers PnP/power callbacks,
/// creates the device with a [`DeviceContext`](device::DeviceContext),
/// and initializes the I/O queues.
///
/// # Safety
///
/// Called by WDF with a valid driver handle and device init structure.
unsafe extern "C" fn evt_driver_device_add(
    _driver: WDFDRIVER,
    mut device_init: PWDFDEVICE_INIT,
) -> NTSTATUS {
    // Mark as a filter driver (lower filter beneath mshidkmdf)
    unsafe {
        call_unsafe_wdf_function_binding!(WdfFdoInitSetFilter, device_init);
    }

    // Register PnP and power event callbacks
    let mut pnp_callbacks: WDF_PNPPOWER_EVENT_CALLBACKS = unsafe { core::mem::zeroed() };
    pnp_callbacks.Size = core::mem::size_of::<WDF_PNPPOWER_EVENT_CALLBACKS>() as ULONG;
    pnp_callbacks.EvtDevicePrepareHardware = Some(evt_device_prepare_hardware);
    pnp_callbacks.EvtDeviceD0Entry = Some(evt_device_d0_entry);
    pnp_callbacks.EvtDeviceD0Exit = Some(evt_device_d0_exit);
    unsafe {
        call_unsafe_wdf_function_binding!(
            WdfDeviceInitSetPnpPowerEventCallbacks,
            device_init,
            &mut pnp_callbacks
        );
    }

    // Prepare device object attributes with DeviceContext
    let mut device_attrs: WDF_OBJECT_ATTRIBUTES = unsafe { core::mem::zeroed() };
    device_attrs.Size = core::mem::size_of::<WDF_OBJECT_ATTRIBUTES>() as ULONG;
    device_attrs.ExecutionLevel =
        WDF_EXECUTION_LEVEL::WdfExecutionLevelInheritFromParent;
    device_attrs.SynchronizationScope =
        WDF_SYNCHRONIZATION_SCOPE::WdfSynchronizationScopeInheritFromParent;
    device_attrs.ContextTypeInfo =
        &DEVICE_CONTEXT_TYPE_INFO as *const WDF_OBJECT_CONTEXT_TYPE_INFO;
    device_attrs.ContextSizeOverride = core::mem::size_of::<device::DeviceContext>();

    // Create the WDF device object
    let mut device: WDFDEVICE = core::ptr::null_mut();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfDeviceCreate,
            &mut device_init,
            &mut device_attrs,
            &mut device
        )
    };
    if !NT_SUCCESS(status) {
        println!("WdfDeviceCreate failed: {status:#x}");
        return status;
    }

    // Initialize device context with safe defaults
    let ctx = get_device_context(device);
    unsafe { (*ctx).init_defaults() };

    // Create I/O queues (default parallel + manual input)
    let status = unsafe { queue_initialize(device) };
    if !NT_SUCCESS(status) {
        println!("QueueInitialize failed: {status:#x}");
        return status;
    }

    STATUS_SUCCESS
}

/// EvtDevicePrepareHardware — USB device setup.
///
/// Called after the PnP manager assigns hardware resources. We create the
/// USB device, discover the trackpad type, select the interface, and
/// configure the continuous reader.
///
/// # Safety
///
/// Called by WDF with valid device handle and resource lists.
unsafe extern "C" fn evt_device_prepare_hardware(
    device: WDFDEVICE,
    _resources_raw: WDFCMRESLIST,
    _resources_translated: WDFCMRESLIST,
) -> NTSTATUS {
    unsafe { prepare_usb_hardware(device) }
}
