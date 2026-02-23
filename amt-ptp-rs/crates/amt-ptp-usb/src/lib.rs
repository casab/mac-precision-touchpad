//! # amt-ptp-usb
//!
//! KMDF USB function driver for Apple Magic Trackpad 2/3.
//!
//! This driver communicates directly with the trackpad over USB:
//! - Selects the correct USB interface and interrupt pipe
//! - Enables Wellspring mode (raw multitouch data) via USB control transfers
//! - Reads touch data from the USB interrupt pipe
//! - Transforms Apple TYPE5 finger data into Windows PTP reports
//! - Serves as a HID minidriver to present a Precision Touchpad to Windows
//!
//! ## Supported Devices
//! - Apple Magic Trackpad 2 (USB, PID 0x0265)
//! - Apple Magic Trackpad 3 (USB, PID TBD)

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(test))]
extern crate wdk_panic;

#[cfg(not(test))]
use wdk_alloc::WdkAllocator;

use wdk_sys::{NTSTATUS, PCUNICODE_STRING, PDRIVER_OBJECT};

#[cfg(not(test))]
#[global_allocator]
static GLOBAL_ALLOCATOR: WdkAllocator = WdkAllocator;

/// Driver entry point.
///
/// Called by Windows when the driver is loaded. Initializes WDF and registers
/// the `EvtDriverDeviceAdd` callback.
///
/// # Safety
///
/// Called by the Windows kernel with valid pointers to the driver object and
/// registry path. Must not be called from user code.
// SAFETY: "DriverEntry" is the required symbol name for Windows driver entry points.
#[export_name = "DriverEntry"]
pub unsafe extern "system" fn driver_entry(
    _driver: PDRIVER_OBJECT,
    _registry_path: PCUNICODE_STRING,
) -> NTSTATUS {
    // Phase 12 will implement: WdfDriverCreate + EvtDriverDeviceAdd registration
    0 // STATUS_SUCCESS
}
