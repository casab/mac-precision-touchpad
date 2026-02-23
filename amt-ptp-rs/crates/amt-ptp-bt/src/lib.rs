//! # amt-ptp-bt
//!
//! KMDF Bluetooth HID filter driver with VHF for Apple Magic Trackpad 2/3.
//!
//! This driver sits as a filter in the Bluetooth HID device stack:
//! - Intercepts raw Apple HID reports from the Bluetooth transport
//! - Creates a virtual PTP (Precision Touchpad) device via VHF
//! - Transforms Apple TYPE5 finger data into Windows PTP reports
//! - Submits PTP reports to the virtual device via `VhfReadReportSubmit`
//! - Handles PTP feature reports (caps, HQA, input mode) via VHF callbacks
//!
//! This replaces the fragile HIDCLASS detour hack from the original C driver
//! with Microsoft's documented Virtual HID Framework (VHF) API.
//!
//! ## Supported Devices
//! - Apple Magic Trackpad 2 (Bluetooth, PID 0x0265)
//! - Apple Magic Trackpad 3 (Bluetooth, PID TBD)

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
/// the `EvtDriverDeviceAdd` callback with `WdfFdoInitSetFilter`.
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
    // Phase 18 will implement: WdfDriverCreate + EvtDriverDeviceAdd with WdfFdoInitSetFilter
    0 // STATUS_SUCCESS
}
