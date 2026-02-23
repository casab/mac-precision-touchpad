//! Build script for vhf-sys.
//!
//! Emits the linker directive for Vhfkm.lib so any crate that depends
//! on vhf-sys automatically links the VHF kernel-mode library.
//!
//! The WDK library search paths are set by the consuming driver crate's
//! build script (via `wdk_build::configure_wdk_binary_build()`).

fn main() {
    // Link against the VHF kernel-mode library from the WDK.
    // This directive propagates to all crates that depend on vhf-sys.
    println!("cargo:rustc-link-lib=Vhfkm");
}
