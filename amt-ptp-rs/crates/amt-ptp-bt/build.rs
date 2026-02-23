//! Build script for the amt-ptp-bt KMDF filter driver.
//!
//! Reads `[package.metadata.wdk.driver-model]` from Cargo.toml and emits
//! all necessary linker flags, include paths, and library links for
//! kernel-mode compilation.

fn main() -> Result<(), wdk_build::ConfigError> {
    wdk_build::configure_wdk_binary_build()
}
