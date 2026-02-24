//! # amt-ptp-core
//!
//! Shared library for the Apple Magic Trackpad Windows Precision Touchpad driver.
//!
//! This crate contains all transport-independent logic:
//! - Device configuration tables (Magic Trackpad 2/3, T2)
//! - TYPE5 finger data parsing (Apple's 9-byte packed format)
//! - Legacy TYPE2-4 finger parsing (Wellspring/T2 28-30 byte format)
//! - Coordinate transformation (Apple raw → PTP coordinate space)
//! - PTP report types (Windows Precision Touchpad HID structures)
//! - HID report descriptor builder
//! - Feature report types (device caps, HQA, input mode, selective reporting)
//! - HQA certification blob
//!
//! This crate is `#![no_std]` so it can be linked into kernel-mode KMDF drivers.
//! It can also be tested in user-mode with standard `cargo test`.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

pub mod constants;
pub mod device;
pub mod error;
pub mod finger;
pub mod hid;
pub mod ptp;
