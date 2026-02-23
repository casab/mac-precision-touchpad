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
//!
//! ## Usage
//!
//! This crate is used by `amt-ptp-bt` to create a virtual Precision Touchpad
//! device that Windows sees as a real PTP touchpad.
//!
//! ## Note
//!
//! Bindings will be populated in Phase 11. Currently this is a placeholder
//! to establish the crate structure and workspace dependency graph.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(missing_docs)] // FFI bindings don't need per-item docs

// Phase 11 will add:
// - VHF_CONFIG struct
// - VHFHANDLE / VHFOPERATIONHANDLE type aliases
// - Callback type aliases (EVT_VHF_ASYNC_OPERATION, etc.)
// - extern "C" block with VhfCreate, VhfStart, VhfReadReportSubmit, etc.
// - VHF_CONFIG_INIT helper function
