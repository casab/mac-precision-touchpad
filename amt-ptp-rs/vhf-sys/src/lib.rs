//! # vhf-sys
//!
//! Raw FFI bindings for the Windows Virtual HID Framework (VHF).
//!
//! VHF (`Vhf.sys` / `Vhfkm.lib`) provides a clean API for creating virtual HID
//! devices in kernel-mode drivers. This crate exposes the C types and functions
//! from `vhf.h` as Rust FFI bindings.
//!
//! ## API Coverage
//!
//! Functions:
//! - [`VhfCreate`] — Create a virtual HID device
//! - [`VhfStart`] — Start the virtual HID device
//! - [`VhfReadReportSubmit`] — Submit an input report to the virtual device
//! - [`VhfAsyncOperationComplete`] — Complete an async feature report operation
//! - [`VhfDelete`] — Delete the virtual HID device
//!
//! Structs:
//! - [`VHF_CONFIG`] — Configuration for virtual HID device creation
//! - [`HID_XFER_PACKET`] — HID transfer packet for reports
//!
//! ## Usage
//!
//! This crate is used by `amt-ptp-bt` to create a virtual Precision Touchpad
//! device that Windows sees as a real PTP touchpad.
//!
//! ## Linking
//!
//! This crate emits `cargo:rustc-link-lib=Vhfkm` from its build script.
//! The WDK library search paths must be set by the consuming driver crate's
//! build script (via `wdk_build::configure_wdk_binary_build()`).

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(missing_docs)] // FFI bindings match Windows naming conventions
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::doc_markdown)]

use core::ffi::c_void;

// ── Re-exported base types from wdk-sys ─────────────────────────────
//
// These are the fundamental Windows kernel types used throughout the
// VHF API surface. Re-exported so consumers don't need to also import
// wdk-sys directly for basic VHF usage.

pub use wdk_sys::{BOOLEAN, GUID, NTSTATUS, ULONG, USHORT};

// ── Pointer type aliases ────────────────────────────────────────────
//
// Defined locally rather than imported from wdk-sys because these are
// simple pointer aliases that are always the same regardless of WDK
// version or bindgen output.

/// Pointer to void (`PVOID` in Windows DDK).
pub type PVOID = *mut c_void;

/// Pointer to unsigned char (`PUCHAR` in Windows DDK).
pub type PUCHAR = *mut u8;

/// Pointer to wide string (`PWSTR` in Windows DDK).
pub type PWSTR = *mut u16;

/// Pointer to WDM `DEVICE_OBJECT`.
///
/// In the BT driver, obtain this from a WDFDEVICE handle via:
/// ```ignore
/// let wdm_device = WdfDeviceWdmGetDeviceObject(wdf_device);
/// ```
///
/// Declared as opaque `*mut c_void` to avoid depending on the full
/// `DEVICE_OBJECT` struct definition from wdk-sys.
pub type PDEVICE_OBJECT = *mut c_void;

// ── VHF Handle Types ────────────────────────────────────────────────

/// Opaque handle to a VHF virtual HID device instance.
///
/// Returned by [`VhfCreate`], used by [`VhfStart`], [`VhfReadReportSubmit`],
/// and [`VhfDelete`].
pub type VHFHANDLE = *mut c_void;

/// Opaque handle to an in-progress VHF async operation.
///
/// Passed to VHF async callbacks ([`EVT_VHF_ASYNC_OPERATION`]) and must be
/// completed via [`VhfAsyncOperationComplete`].
pub type VHFOPERATIONHANDLE = *mut c_void;

// ── HID Transfer Packet ─────────────────────────────────────────────

/// HID transfer packet for report data exchange.
///
/// Used by VHF callbacks to pass feature report data and by
/// [`VhfReadReportSubmit`] to submit input reports.
///
/// Matches `HID_XFER_PACKET` from `hidport.h`.
///
/// ## Layout (C equivalent)
/// ```c
/// typedef struct _HID_XFER_PACKET {
///     PUCHAR  reportBuffer;
///     ULONG   reportBufferLen;
///     UCHAR   reportId;
/// } HID_XFER_PACKET, *PHID_XFER_PACKET;
/// ```
#[repr(C)]
pub struct HID_XFER_PACKET {
    /// Pointer to the report data buffer.
    /// For input reports, this contains the report to submit.
    /// For feature reports, this is filled by the callback.
    pub reportBuffer: *mut u8,
    /// Length of the report buffer in bytes.
    pub reportBufferLen: ULONG,
    /// HID report ID.
    pub reportId: u8,
}

// ── VHF Callback Type Aliases ───────────────────────────────────────
//
// These match the callback signatures defined in `vhf.h`.
// The driver implements these and registers them in VHF_CONFIG.

/// Callback for async VHF operations (GET_FEATURE, SET_FEATURE, WRITE_REPORT,
/// GET_INPUT_REPORT).
///
/// The driver must complete the operation by calling [`VhfAsyncOperationComplete`]
/// with the operation handle and a status code.
///
/// ## Parameters
/// - `VhfClientContext` — driver-defined context (from `VHF_CONFIG.VhfClientContext`)
/// - `VhfOperationHandle` — handle to complete via [`VhfAsyncOperationComplete`]
/// - `VhfOperationContext` — per-operation context (size set in `VHF_CONFIG.OperationContextSize`)
/// - `HidTransferPacket` — the HID report data
pub type EVT_VHF_ASYNC_OPERATION = unsafe extern "C" fn(
    VhfClientContext: PVOID,
    VhfOperationHandle: VHFOPERATIONHANDLE,
    VhfOperationContext: PVOID,
    HidTransferPacket: *mut HID_XFER_PACKET,
);

/// Callback invoked when VHF is ready to accept the next input report.
///
/// The driver should call [`VhfReadReportSubmit`] from this callback
/// or set a flag to submit the report later from the transport callback.
///
/// ## Parameters
/// - `VhfClientContext` — driver-defined context (from `VHF_CONFIG.VhfClientContext`)
pub type EVT_VHF_READY_FOR_NEXT_READ_REPORT = unsafe extern "C" fn(
    VhfClientContext: PVOID,
);

/// Cleanup callback invoked when the VHF device is being deleted.
///
/// The driver should release any resources associated with the VHF device.
///
/// ## Parameters
/// - `VhfClientContext` — driver-defined context (from `VHF_CONFIG.VhfClientContext`)
pub type EVT_VHF_CLEANUP = unsafe extern "C" fn(
    VhfClientContext: PVOID,
);

// ── VHF_CONFIG ──────────────────────────────────────────────────────

/// Configuration structure for creating a VHF virtual HID device.
///
/// Passed to [`VhfCreate`] to define the virtual device's properties,
/// HID report descriptor, and callback functions.
///
/// Initialize with [`VHF_CONFIG::init`] (equivalent to the C macro
/// `VHF_CONFIG_INIT`), then set additional fields as needed.
///
/// ## C equivalent
/// ```c
/// typedef struct _VHF_CONFIG {
///     ULONG   Size;
///     PVOID   VhfClientContext;
///     ULONG   OperationContextSize;
///     PDEVICE_OBJECT DeviceObject;
///     USHORT  VendorID;
///     USHORT  ProductID;
///     USHORT  VersionNumber;
///     GUID    ContainerID;
///     USHORT  InstanceIDLength;
///     PWSTR   InstanceID;
///     USHORT  ReportDescriptorLength;
///     PUCHAR  ReportDescriptor;
///     PFN_VHF_READY_FOR_NEXT_READ_REPORT EvtVhfReadyForNextReadReport;
///     PFN_VHF_ASYNC_OPERATION EvtVhfAsyncOperationGetFeature;
///     PFN_VHF_ASYNC_OPERATION EvtVhfAsyncOperationSetFeature;
///     PFN_VHF_ASYNC_OPERATION EvtVhfAsyncOperationWriteReport;
///     PFN_VHF_ASYNC_OPERATION EvtVhfAsyncOperationGetInputReport;
///     PFN_VHF_CLEANUP EvtVhfCleanup;
///     USHORT  HardwareIDsLength;
///     PWSTR   HardwareIDs;
/// } VHF_CONFIG, *PVHF_CONFIG;
/// ```
#[repr(C)]
pub struct VHF_CONFIG {
    /// Structure size (must be `size_of::<VHF_CONFIG>()`).
    pub Size: ULONG,
    /// Driver-defined context pointer passed to all callbacks.
    pub VhfClientContext: PVOID,
    /// Size in bytes of per-operation context for async callbacks.
    /// Set to 0 if not needed.
    pub OperationContextSize: ULONG,
    /// WDM device object for the virtual HID device.
    /// Obtain from `WdfDeviceWdmGetDeviceObject()`.
    pub DeviceObject: PDEVICE_OBJECT,
    /// USB Vendor ID reported by the virtual HID device.
    pub VendorID: USHORT,
    /// USB Product ID reported by the virtual HID device.
    pub ProductID: USHORT,
    /// Device version number reported by the virtual HID device.
    pub VersionNumber: USHORT,
    /// Container ID GUID (optional, zeroed if not used).
    pub ContainerID: GUID,
    /// Length of the instance ID string in bytes (0 if not used).
    pub InstanceIDLength: USHORT,
    /// Instance ID wide string (null if not used).
    pub InstanceID: PWSTR,
    /// Length of the HID report descriptor in bytes.
    pub ReportDescriptorLength: USHORT,
    /// Pointer to the HID report descriptor.
    pub ReportDescriptor: PUCHAR,
    /// Called when VHF is ready for the next input report.
    pub EvtVhfReadyForNextReadReport: Option<EVT_VHF_READY_FOR_NEXT_READ_REPORT>,
    /// Called for GET_FEATURE requests from the HID class driver.
    pub EvtVhfAsyncOperationGetFeature: Option<EVT_VHF_ASYNC_OPERATION>,
    /// Called for SET_FEATURE requests from the HID class driver.
    pub EvtVhfAsyncOperationSetFeature: Option<EVT_VHF_ASYNC_OPERATION>,
    /// Called for WRITE_REPORT requests (output reports).
    pub EvtVhfAsyncOperationWriteReport: Option<EVT_VHF_ASYNC_OPERATION>,
    /// Called for GET_INPUT_REPORT requests.
    pub EvtVhfAsyncOperationGetInputReport: Option<EVT_VHF_ASYNC_OPERATION>,
    /// Called during VHF device cleanup/deletion.
    pub EvtVhfCleanup: Option<EVT_VHF_CLEANUP>,
    /// Length of the hardware IDs multi-string in bytes (0 if not used).
    pub HardwareIDsLength: USHORT,
    /// Hardware IDs multi-string (null if not used).
    pub HardwareIDs: PWSTR,
}

impl VHF_CONFIG {
    /// Initialize a VHF_CONFIG with required fields, zeroing everything else.
    ///
    /// Equivalent to the C macro `VHF_CONFIG_INIT(Config, DeviceObject, DescLen, Desc)`.
    ///
    /// After calling this, set additional fields (callbacks, VID/PID, etc.)
    /// before passing to [`VhfCreate`].
    ///
    /// # Safety
    ///
    /// `device_object` must be a valid WDM `PDEVICE_OBJECT`.
    /// `report_descriptor` must point to a valid HID report descriptor buffer
    /// that outlives the VHF device.
    #[must_use]
    pub unsafe fn init(
        device_object: PDEVICE_OBJECT,
        report_descriptor: PUCHAR,
        report_descriptor_length: USHORT,
    ) -> Self {
        // SAFETY: VHF_CONFIG is repr(C) with all integer/pointer fields.
        // Zeroing is safe and equivalent to VHF_CONFIG_INIT's RtlZeroMemory.
        let mut config: Self = unsafe { core::mem::zeroed() };
        config.Size = core::mem::size_of::<Self>() as ULONG;
        config.DeviceObject = device_object;
        config.ReportDescriptor = report_descriptor;
        config.ReportDescriptorLength = report_descriptor_length;
        config
    }
}

// ── VHF API Functions ───────────────────────────────────────────────
//
// Linked from Vhfkm.lib (WDK library). The build.rs emits
// `cargo:rustc-link-lib=Vhfkm` so consumers auto-link.
//
// All functions match the signatures in `vhf.h`.

extern "C" {
    /// Create a virtual HID device.
    ///
    /// Allocates and initializes a VHF device based on the provided config.
    /// On success, `VhfHandle` receives the device handle.
    ///
    /// Must be followed by [`VhfStart`] to make the device visible to Windows.
    ///
    /// # Safety
    ///
    /// - `VhfConfig` must point to a valid, initialized [`VHF_CONFIG`]
    /// - `VhfHandle` must point to a valid `VHFHANDLE` storage location
    pub fn VhfCreate(VhfConfig: *mut VHF_CONFIG, VhfHandle: *mut VHFHANDLE) -> NTSTATUS;

    /// Start the virtual HID device.
    ///
    /// Makes the VHF device visible to the HID class driver and Windows.
    /// After this call, callbacks may be invoked.
    ///
    /// # Safety
    ///
    /// `VhfHandle` must be a valid handle returned by [`VhfCreate`].
    pub fn VhfStart(VhfHandle: VHFHANDLE) -> NTSTATUS;

    /// Submit an input (read) report to the virtual HID device.
    ///
    /// Called by the driver when new touch data is available. VHF forwards
    /// the report to the HID class driver as if it came from real hardware.
    ///
    /// # Safety
    ///
    /// - `VhfHandle` must be a valid, started VHF handle
    /// - `HidTransferPacket` must point to a valid [`HID_XFER_PACKET`] with
    ///   a valid report buffer
    pub fn VhfReadReportSubmit(
        VhfHandle: VHFHANDLE,
        HidTransferPacket: *mut HID_XFER_PACKET,
    ) -> NTSTATUS;

    /// Complete an async VHF operation.
    ///
    /// Must be called from async callbacks ([`EVT_VHF_ASYNC_OPERATION`]) to
    /// signal completion of GET_FEATURE, SET_FEATURE, etc.
    ///
    /// # Safety
    ///
    /// `VhfOperationHandle` must be a valid handle received in the callback.
    pub fn VhfAsyncOperationComplete(
        VhfOperationHandle: VHFOPERATIONHANDLE,
        CompletionStatus: NTSTATUS,
    ) -> NTSTATUS;

    /// Delete the virtual HID device.
    ///
    /// If `Wait` is `TRUE`, blocks until all pending operations complete.
    /// If `FALSE`, returns immediately and the cleanup callback is invoked later.
    ///
    /// # Safety
    ///
    /// `VhfHandle` must be a valid VHF handle. After this call, the handle
    /// is invalid and must not be used.
    pub fn VhfDelete(VhfHandle: VHFHANDLE, Wait: BOOLEAN);
}
