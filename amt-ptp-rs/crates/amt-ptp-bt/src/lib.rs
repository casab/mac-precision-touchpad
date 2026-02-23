//! # amt-ptp-bt
//!
//! KMDF Bluetooth HID filter driver with VHF for Apple Magic Trackpad 2/3.
//!
//! This driver sits as a lower filter in the Bluetooth HID device stack:
//! - Intercepts raw Apple HID reports from the Bluetooth transport
//! - Creates a virtual PTP (Precision Touchpad) device via VHF
//! - Transforms Apple TYPE5 finger data into Windows PTP reports
//! - Submits PTP reports to the virtual device via `VhfReadReportSubmit`
//! - Handles PTP feature reports (caps, HQA, input mode) via VHF callbacks
//!
//! This replaces the fragile HIDCLASS detour hack from the original C driver
//! with Microsoft's documented Virtual HID Framework (VHF) API.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────┐
//! │      Windows HID Class       │  ← sees our VHF virtual PTP device
//! ├──────────────────────────────┤
//! │    VHF (Virtual HID FW)      │  ← we create/manage this
//! ├──────────────────────────────┤
//! │  amt_ptp_bt (this driver)    │  ← filter driver
//! ├──────────────────────────────┤
//! │    BT HID Transport          │  ← underlying BT device
//! └──────────────────────────────┘
//! ```
//!
//! ## Supported Devices
//! - Apple Magic Trackpad 2 (Bluetooth, PID 0x0265)
//! - Apple Magic Trackpad 3 (Bluetooth, PID TBD — Phase 10)

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
mod self_managed_io;
mod transport;
mod vhf_device;

use device::{get_device_context, DEVICE_CONTEXT_TYPE_INFO};
use self_managed_io::{
    evt_self_managed_io_cleanup, evt_self_managed_io_init,
    evt_self_managed_io_restart, evt_self_managed_io_suspend,
};

/// Driver entry point.
///
/// Initializes WDF, registers [`evt_driver_device_add`], and returns.
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
/// Sets up the device as a filter driver (lower filter in the BT HID stack),
/// registers PnP/power callbacks including self-managed I/O for VHF lifecycle,
/// creates the device with a [`DeviceContext`](device::DeviceContext), and
/// obtains the WDM device object for VHF.
///
/// ## Callback Registration
///
/// - **EvtDeviceSelfManagedIoInit**: Create and start VHF virtual PTP device
/// - **EvtDeviceSelfManagedIoRestart**: Re-configure after suspend
/// - **EvtDeviceSelfManagedIoSuspend**: Prepare for power-down
/// - **EvtDeviceSelfManagedIoCleanup**: Delete VHF device
///
/// # Safety
///
/// Called by WDF with a valid driver handle and device init structure.
unsafe extern "C" fn evt_driver_device_add(
    _driver: WDFDRIVER,
    mut device_init: PWDFDEVICE_INIT,
) -> NTSTATUS {
    // Mark as a filter driver (lower filter in the BT HID device stack).
    // As a filter, we don't own power policy and pass through unhandled I/O.
    unsafe {
        call_unsafe_wdf_function_binding!(WdfFdoInitSetFilter, device_init);
    }

    // Register PnP, power, and self-managed I/O callbacks
    let mut pnp_callbacks: WDF_PNPPOWER_EVENT_CALLBACKS = unsafe { core::mem::zeroed() };
    pnp_callbacks.Size =
        core::mem::size_of::<WDF_PNPPOWER_EVENT_CALLBACKS>() as ULONG;

    // Self-managed I/O: VHF lifecycle
    pnp_callbacks.EvtDeviceSelfManagedIoInit = Some(evt_self_managed_io_init);
    pnp_callbacks.EvtDeviceSelfManagedIoRestart = Some(evt_self_managed_io_restart);
    pnp_callbacks.EvtDeviceSelfManagedIoSuspend = Some(evt_self_managed_io_suspend);
    pnp_callbacks.EvtDeviceSelfManagedIoCleanup = Some(evt_self_managed_io_cleanup);

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
    let mut wdf_device: WDFDEVICE = core::ptr::null_mut();
    let status = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfDeviceCreate,
            &mut device_init,
            &mut device_attrs,
            &mut wdf_device
        )
    };
    if !NT_SUCCESS(status) {
        println!("WdfDeviceCreate failed: {status:#x}");
        return status;
    }

    // Initialize device context with safe defaults
    let ctx = unsafe { &mut *get_device_context(wdf_device) };
    unsafe { ctx.init_defaults() };

    // Store handles needed by other callbacks
    ctx.device = wdf_device;

    // Get the WDM device object for VHF_CONFIG.DeviceObject.
    // SAFETY: WdfDeviceWdmGetDeviceObject returns the underlying WDM device
    // object for a valid WDFDEVICE handle.
    ctx.wdm_device_object = unsafe {
        call_unsafe_wdf_function_binding!(WdfDeviceWdmGetDeviceObject, wdf_device)
    }
    .cast();

    if ctx.wdm_device_object.is_null() {
        println!("WdfDeviceWdmGetDeviceObject returned NULL");
        return STATUS_UNSUCCESSFUL;
    }

    println!("BT filter device created successfully");
    STATUS_SUCCESS
}
