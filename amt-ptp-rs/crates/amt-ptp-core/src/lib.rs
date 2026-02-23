//! # amt-ptp-core
//!
//! Shared library for the Apple Magic Trackpad Windows Precision Touchpad driver.
//!
//! This crate contains all transport-independent logic:
//! - Device configuration tables (Magic Trackpad 2/3)
//! - TYPE5 finger data parsing (Apple's 9-byte packed format)
//! - Coordinate transformation (Apple raw → PTP coordinate space)
//! - PTP report generation (Windows Precision Touchpad HID reports)
//! - HID report descriptor builder
//! - Feature report handling (device caps, HQA, input mode, etc.)
//! - Scan time calculation
//!
//! This crate is `#![no_std]` so it can be linked into kernel-mode KMDF drivers.
//! It can also be tested in user-mode with standard `cargo test`.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

// Modules will be added in Phases 3-9:
// pub mod constants;
// pub mod device;
// pub mod error;
// pub mod feature;
// pub mod finger;
// pub mod hid_descriptor;
// pub mod ptp;
// pub mod time;
// pub mod transform;
