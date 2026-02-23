# Magic Trackpad 2/3 — Rust Driver Rewrite Plan

## Executive Summary

Rewrite the Windows Precision Touchpad driver for Apple Magic Trackpad 2 and 3 in Rust,
targeting USB and Bluetooth transports. Uses `windows-drivers-rs` for WDF bindings and
the Virtual HID Framework (VHF) to replace the fragile HIDCLASS detour hack.

---

## Implementation Progress

| Phase | Description | Status | Lines | Tests | Commit |
|-------|-------------|--------|-------|-------|--------|
| 1 | Workspace scaffolding | **DONE** | ~200 | — | `e5b622a` |
| 2 | Build system & INX driver files | **DONE** | ~500 | — | `9772189` |
| 3 | Core library (device, finger, ptp, hid, error) | **DONE** | ~1,600 | 33 pass | `d2b242c` |
| 4 | USB driver (full HID miniport, 7 modules) | **DONE** | ~1,400 | — | `958b09d` |
| 4.5 | USB driver code review & bug fixes | **DONE** | — | 33 pass | — |
| 5 | USB driver on-device testing & packaging | **MANUAL** | — | — | — |
| 6 | VHF-sys FFI bindings | Pending | — | — | — |
| 7 | BT driver skeleton + VHF | Pending | — | — | — |
| 8 | BT driver transport & input | Pending | — | — | — |
| 9 | BT driver testing & recovery | Pending | — | — | — |
| 10 | MT3 support, settings app, packaging | Pending | — | — | — |

### Phase Consolidation Notes

The original 24-phase plan was consolidated into 10 focused phases during
implementation to reduce overhead and deliver larger coherent units:

- **Phase 1** = Original Phase 1 (workspace scaffolding)
- **Phase 2** = Original Phase 2 + INX files for both drivers + unified package INX
- **Phase 3** = Original Phases 3-10 (entire `amt-ptp-core`: device configs for
  6 devices including MT2 + all T2 variants, TYPE5 + TYPE2-4 finger parsing,
  coordinate transforms, PTP report structs, HID descriptor builder,
  feature reports, HQA blob, constants, error types — 30 unit tests passing)
- **Phase 4** = Original Phases 12-16 (entire USB driver: DriverEntry,
  EvtDriverDeviceAdd, USB init, Wellspring mode, continuous reader,
  power management, queue setup, HID IOCTL dispatch, feature report handlers,
  touch input processing → PTP reports)

Remaining phases:
- **Phase 4.5** = Code review + bug fixes (scan time, selective reporting, physical max)
- **Phase 5** = Original Phase 17 (USB build verification + on-device testing) — **MANUAL, non-blocking until Phase 9**
- **Phase 6** = Original Phase 11 (VHF FFI bindings crate) — **NEXT**
- **Phases 7-9** = Original Phases 18-23 (BT driver)
- **Phase 10** = Original Phase 24 (MT3, settings app, final packaging)

---

## 1. Technology Stack

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Language | Rust | Safety, modern tooling, user preference |
| WDF Bindings | `windows-drivers-rs` (`wdk-sys 0.5.1` + `wdk 0.4.1`) | Official Microsoft crate, production-proven on Surface hardware |
| Driver Model | KMDF (both drivers) | VHF requires kernel-mode; consistency between USB and BT drivers |
| BT HID Virtualization | VHF (Virtual HID Framework) | Documented API, replaces fragile HIDCLASS detour hack |
| Build System | `cargo-make` + `cargo-wdk` + WDK 22H2 | Standard Rust toolchain with WDK integration |
| Min Windows Version | Windows 11 22H2 (build 22621) | Exact match with `windows-drivers-rs` default; zero WDK friction |
| Min WDF Version | KMDF 1.33 | Default for WDK 22H2, no manual `wdk-sys` patching needed |
| Rust Toolchain | `nightly-2025-02-10` | Pinned for reproducibility; required for `#![no_std]` kernel drivers |

### Build Prerequisites

- Windows 11 22H2+ (build machine)
- Enterprise WDK (eWDK) 22H2 or newer
- Visual Studio 2022 with "Desktop development with C++" workload
- LLVM 17.0.6 (for bindgen)
- Rust nightly toolchain pinned to `nightly-2025-02-10`
- `cargo-wdk` (`cargo install cargo-wdk`)
- `cargo-make` (`cargo install cargo-make`) — build automation

---

## 2. Project Structure (Implemented)

```
amt-ptp-rs/
├── Cargo.toml                    # Workspace root (KMDF 1.33 metadata)
├── rust-toolchain.toml           # Pinned nightly-2025-02-10
├── .cargo/config.toml            # +crt-static rustflag for kernel mode
├── .gitignore
├── Makefile.toml                 # cargo-make build automation
├── crates/
│   ├── amt-ptp-core/             # Shared #![no_std] library (rlib)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            # Module declarations, extern crate alloc
│   │       ├── constants.rs      # All hardware constants, report IDs, thresholds
│   │       ├── device.rs         # DeviceConfig, TrackpadType, CONFIG_TABLE (6 entries)
│   │       ├── error.rs          # Error enum (BufferTooShort, MalformedPayload, etc.)
│   │       ├── finger.rs         # TYPE5 + TYPE2-4 finger parsing, coordinate transform
│   │       ├── hid.rs            # HID report descriptor builder, DEFAULT_HQA_BLOB
│   │       └── ptp.rs            # PTP report structs (50B input, feature reports)
│   │
│   ├── amt-ptp-usb/              # USB KMDF lower-filter driver (cdylib)
│   │   ├── Cargo.toml            # wdk-sys features: usb, hid
│   │   ├── build.rs              # wdk-build configuration
│   │   ├── amt_ptp_usb.inx       # INF template (14 HW IDs: 13 T2 + MT2)
│   │   └── src/
│   │       ├── lib.rs            # DriverEntry → WdfDriverCreate → EvtDriverDeviceAdd
│   │       ├── device.rs         # DeviceContext struct, WDF context accessor
│   │       ├── usb.rs            # USB hardware init, pipe selection, Wellspring mode
│   │       ├── power.rs          # D0Entry (start reader) / D0Exit (stop + disable)
│   │       ├── queue.rs          # Default parallel queue + manual input queue
│   │       ├── hid.rs            # HID descriptor, attributes, report desc, features
│   │       └── input.rs          # Continuous reader → finger parse → PTP → complete
│   │
│   ├── amt-ptp-bt/               # Bluetooth KMDF filter driver + VHF (stub)
│   │   ├── Cargo.toml            # wdk-sys features: hid
│   │   ├── build.rs
│   │   ├── amt_ptp_bt.inx        # BT filter INF (Col01 MT2, null Col02/Col03)
│   │   └── src/
│   │       └── lib.rs            # Stub DriverEntry
│   │
│   └── amt-ptp-settings/         # Settings app (Phase 10)
│       └── ...
│
├── vhf-sys/                      # VHF FFI bindings placeholder
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs                # Stub
│
└── pkg/
    └── AmtPtpDevice.inx          # Unified package INF (both .sys drivers)
```

### Workspace Cargo.toml (Actual)

```toml
[workspace]
members = [
    "crates/amt-ptp-core",
    "crates/amt-ptp-usb",
    "crates/amt-ptp-bt",
    "vhf-sys",
]
resolver = "2"

[workspace.package]
edition = "2021"
license = "GPL-2.0"
repository = "https://github.com/casab/mac-precision-touchpad"

[workspace.metadata.wdk.driver-model]
driver-type = "KMDF"
kmdf-version-major = 1
target-kmdf-version-minor = 33

[workspace.dependencies]
wdk = "0.4.1"
wdk-sys = "0.5.1"
wdk-alloc = "0.4.1"
wdk-panic = "0.4.1"
wdk-build = "0.5.1"
amt-ptp-core = { path = "crates/amt-ptp-core" }
vhf-sys = { path = "vhf-sys" }

[workspace.lints.rust]
missing_docs = "warn"
unsafe_op_in_unsafe_fn = "forbid"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
```

---

## 3. Crate Design: `amt-ptp-core` (Implemented)

The shared `#![no_std]` library containing all transport-independent logic.
**30 unit tests passing.** Modules: `constants`, `device`, `error`, `finger`, `hid`, `ptp`.

### 3.1 Device Configuration (`device.rs`)

Supports 6 device configs: 1 fallback + 4 T2 variants + Magic Trackpad 2.
Lookup by USB PID with automatic fallback for unknown T2 devices.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrackpadType { Type1, Type2, Type3, Type4, Type5 }

impl TrackpadType {
    pub const fn header_size_usb(self) -> usize;  // 12 for TYPE5, 46 for TYPE4
    pub const fn header_size_bt(self) -> usize;   // 4 for TYPE5 BT
    pub const fn finger_size(self) -> usize;      // 9 for TYPE5, 30 for TYPE4
    pub const fn finger_delta(self) -> usize;     // Offset from header to first finger
    pub const fn button_offset(self) -> usize;    // 1 for TYPE5, 31 for TYPE4
    pub const fn usb_report_size(self) -> usize;  // header + MAX_FINGERS * finger_size
}

pub struct DeviceConfig {
    pub product_id: u16,
    pub caps: u32,
    pub trackpad_type: TrackpadType,
    pub wellspring_msg: WellspringMsg,
    pub pressure: AxisParams,
    pub width: AxisParams,
    pub x: AxisParams,              // e.g. MT2: min=-3678, max=3934
    pub y: AxisParams,              // e.g. MT2: min=-2479, max=2586
    pub orientation: AxisParams,
    // ... button_endpoint, button_data_len, trackpad_endpoint
}

pub static CONFIG_TABLE: &[DeviceConfig] = &[/* 6 entries */];
pub fn lookup_config(product_id: u16) -> &'static DeviceConfig;
```

### 3.2 TYPE5 Finger Parsing (`finger.rs`)

Supports both TYPE5 (MT2, 9-byte packed) and TYPE2-4 (T2, 28-30 byte le16-aligned).
Parsed into a common `Finger` struct. Y-axis inversion handled per type.

```rust
pub struct Finger {
    pub raw_x: i32, pub raw_y: i32,
    pub touch_major: u8, pub touch_minor: u8,
    pub size: u8, pub pressure: u8,
    pub contact_id: u8, pub orientation: u8,
}

impl Finger {
    pub fn transform_to_ptp(&self, config: &DeviceConfig) -> (u16, u16);
    pub const fn is_tip_switch(&self) -> bool;    // (touch_major << 1) > 0
    pub const fn is_confident(&self) -> bool;     // (touch_minor << 1) < 345
}

pub fn parse_type5_finger(data: &[u8; 9]) -> Finger;          // 13-bit signed X/Y
pub fn parse_legacy_finger(data: &[u8], config: &DeviceConfig) -> Finger;
pub fn parse_report(report: &[u8], config: &DeviceConfig, header_size: usize,
                    out: &mut [Finger]) -> Result<(usize, bool), Error>;
```

### 3.3 PTP Report Types (`ptp.rs`)

All `#[repr(C, packed)]` with verified sizes via unit tests:

| Struct | Size | Report ID |
|--------|------|-----------|
| `PtpContact` | 9B | — |
| `PtpReport` | 50B | 0x05 |
| `PtpDeviceCapsReport` | 3B | 0x07 |
| `PtpInputModeReport` | 2B | 0x04 |
| `PtpSelectiveReportingReport` | 2B | 0x06 |
| `PtpHqaCertificationReport` | 257B | 0x08 |
| `PtpUserModeAppConfReport` | 4B | 0x09 |

### 3.4 HID Report Descriptor (`hid.rs`)

Dynamic builder parameterized by device config's coordinate ranges:

```rust
pub fn build_report_descriptor(
    x_logical_max: u16, y_logical_max: u16,
    x_physical_max: u16, y_physical_max: u16,
) -> alloc::vec::Vec<u8>;
```

Generates 3 top-level collections: Digitizer Touch Pad (5 fingers + scan time +
contact count + button + device caps + HQA), Configuration (input mode +
selective reporting), Vendor App Config.

Also contains `DEFAULT_HQA_BLOB: [u8; 256]` — the PTP certification blob.

### 3.5 Constants (`constants.rs`) & Errors (`error.rs`)

All hardware constants, report IDs, Apple PIDs, thresholds. Error enum with
`BufferTooShort`, `MalformedPayload`, `FingerIndexOutOfRange` variants.

---

## 4. USB Driver: `amt-ptp-usb` (Implemented)

KMDF lower-filter driver beneath `mshidkmdf` for Magic Trackpad 2/3 and T2 trackpads.
**7 modules, ~1,400 lines.** Full HID miniport implementation.

### 4.1 Module Structure

| Module | Lines | Purpose |
|--------|-------|---------|
| `lib.rs` | 164 | `DriverEntry` → `WdfDriverCreate` → `EvtDriverDeviceAdd` (filter, PnP/power, queues) |
| `device.rs` | 114 | `DeviceContext` struct (USB handles, PTP state, timing), WDF context accessor |
| `usb.rs` | 325 | `prepare_usb_hardware()`, pipe selection, Wellspring mode, continuous reader config |
| `power.rs` | 106 | `evt_device_d0_entry` (start reader) / `evt_device_d0_exit` (stop + disable) |
| `queue.rs` | 165 | Default parallel queue (HID IOCTLs) + manual input queue, IOCTL dispatch |
| `hid.rs` | 393 | HID descriptor, device attributes, report descriptor, GET/SET feature |
| `input.rs` | 147 | Continuous reader callback → finger parse → PTP report → complete request |

### 4.2 Driver Lifecycle (Actual)

```
DriverEntry (lib.rs)
  → WdfDriverCreate(EvtDriverDeviceAdd)

EvtDriverDeviceAdd (lib.rs)
  → WdfFdoInitSetFilter              # lower filter beneath mshidkmdf
  → WdfDeviceInitSetPnpPowerEventCallbacks(PrepareHardware, D0Entry, D0Exit)
  → WdfDeviceCreate with DeviceContext
  → queue_initialize():
    → Default parallel queue (EvtIoInternalDeviceControl)
    → Manual input queue (PowerManaged = WdfFalse)

EvtDevicePrepareHardware (lib.rs → usb.rs)
  → WdfUsbTargetDeviceCreate
  → WdfUsbTargetDeviceGetDeviceDescriptor → lookup_config(PID)
  → WdfUsbTargetDeviceSelectConfig (single interface)
  → Find interrupt IN pipe → WdfUsbTargetPipeSetNoMaximumPacketSizeCheck
  → WDF_USB_CONTINUOUS_READER_CONFIG_INIT → WdfUsbTargetPipeConfigContinuousReader

EvtDeviceD0Entry (power.rs)
  → set_wellspring_mode(ctx, true)
  → KeQueryPerformanceCounter → ctx.last_report_time
  → WdfIoTargetStart(interrupt pipe)

EvtDeviceD0Exit (power.rs)
  → WdfIoTargetStop(WdfIoTargetCancelSentIo)
  → set_wellspring_mode(ctx, false)

EvtIoInternalDeviceControl (queue.rs → hid.rs)
  IOCTL_HID_GET_DEVICE_DESCRIPTOR  → 9-byte HID descriptor
  IOCTL_HID_GET_DEVICE_ATTRIBUTES  → VID/PID/version from USB descriptor
  IOCTL_HID_GET_REPORT_DESCRIPTOR  → Dynamic descriptor from build_report_descriptor()
  IOCTL_HID_READ_REPORT            → Forward to manual input queue
  IOCTL_HID_GET_FEATURE            → Device caps (0x07) or HQA blob (0x08)
  IOCTL_HID_SET_FEATURE            → Input mode (0x04) or selective reporting (0x06)

EvtUsbInterruptPipeReadComplete (input.rs)
  → WdfMemoryGetBuffer → raw_data slice
  → parse_report(raw_data, config, header_size, &mut fingers)
  → KeQueryPerformanceCounter → scan_time delta
  → Build PtpReport (contacts, scan_time, button)
  → WdfIoQueueRetrieveNextRequest(input_queue)
  → WdfMemoryCopyFromBuffer → WdfRequestComplete
```

### 4.3 DeviceContext (device.rs)

```rust
#[repr(C)]
pub struct DeviceContext {
    pub usb_device: WDFUSBDEVICE,
    pub interrupt_pipe: WDFUSBPIPE,
    pub usb_interface: WDFUSBINTERFACE,
    pub device_descriptor: USB_DEVICE_DESCRIPTOR,
    pub usb_device_traits: ULONG,
    pub input_queue: WDFQUEUE,
    pub device_info: Option<&'static DeviceConfig>,
    pub is_wellspring_mode_on: bool,
    pub ptp_input_on: bool,
    pub ptp_report_touch: bool,      // default: true
    pub ptp_report_button: bool,     // default: true
    pub last_report_time: i64,
}
```

### 4.4 Wellspring Mode (usb.rs)

Read-modify-write via `WdfUsbTargetDeviceSendControlTransferSynchronously`:
1. READ current mode (BMREQUEST_DEVICE_TO_HOST, request ID 1)
2. Modify `buf[switch_idx]` = `switch_on` or `switch_off`
3. WRITE modified mode (BMREQUEST_HOST_TO_DEVICE, request ID 9)

Parameters come from `DeviceConfig.wellspring_msg` (size, req_val, req_idx, switch_idx).

---

## 5. Bluetooth Driver: `amt-ptp-bt`

KMDF filter driver with VHF for Magic Trackpad 2/3 connected via Bluetooth.

### 5.1 Architecture (VHF-based, replacing HIDCLASS detour)

```
Application layer (Windows PTP client)
    ↕
HIDCLASS.sys ← sees virtual PTP device
    ↕
MsHidKmdf.sys
    ↕
VHF virtual device PDO ← created by our driver
    ↕ (VhfReadReportSubmit)
Vhf.sys (lower filter)
    ↕
┌──────────────────────────────────┐
│  amt-ptp-bt.sys (our driver)     │ ← KMDF filter, FDO in real BT stack
│  - Intercepts raw BT HID reports │
│  - Transforms to PTP format      │
│  - Submits to VHF virtual device  │
└──────────────────────────────────┘
    ↕
Real BT HID transport
    ↕
Magic Trackpad 2/3 (hardware)
```

### 5.2 Driver Lifecycle

```
DriverEntry
  → WdfDriverCreate
  → EvtDriverDeviceAdd (with WdfFdoInitSetFilter)
    → WdfDeviceCreate
    → Configure self-managed I/O target for BT transport
    → Create WDF queue for internal dispatch

EvtDeviceSelfManagedIoInit
  → Identify trackpad model (read VID/PID from lower device)
  → Create VHF virtual device:
    VHF_CONFIG_INIT(&config, wdm_device, ptp_descriptor, descriptor_len)
    config.VendorID = 0x004C
    config.ProductID = 0x0265
    config.EvtVhfAsyncOperationGetFeature = on_get_feature
    config.EvtVhfAsyncOperationSetFeature = on_set_feature
    VhfCreate(&config, &vhf_handle)
    VhfStart(vhf_handle)
  → Enable multitouch mode (send feature report 0xF1 to BT transport)
  → Start issuing IOCTL_HID_READ_REPORT to lower device

BT Read Completion Callback
  → Parse BT header (4 bytes)
  → Extract button state
  → Call amt_ptp_core::transform_fingers()
  → Calculate scan time
  → Build HID_XFER_PACKET with PTP report
  → VhfReadReportSubmit(vhf_handle, &xfer_packet)
  → Re-issue read request to BT transport

on_get_feature (VHF callback)
  → Switch on report_id:
    REPORTID_DEVICE_CAPS → fill DeviceCapsReport
    REPORTID_PTPHQA      → fill HqaCertReport
    REPORTID_REPORTMODE  → fill InputModeReport
    REPORTID_FUNCSWITCH  → fill FunctionSwitchReport
  → VhfAsyncOperationComplete(handle, STATUS_SUCCESS)

on_set_feature (VHF callback)
  → Switch on report_id:
    REPORTID_REPORTMODE  → store input mode
    REPORTID_FUNCSWITCH  → store button/surface enable state
  → VhfAsyncOperationComplete(handle, STATUS_SUCCESS)

EvtDeviceSelfManagedIoCleanup
  → VhfDelete(vhf_handle, wait=true)
  → Stop read requests to BT transport
```

### 5.3 VHF FFI Bindings (`vhf-sys` crate)

Since `wdk-sys` doesn't include VHF headers, we create a standalone crate:

```rust
// vhf-sys/src/lib.rs
#![no_std]

use wdk_sys::*;

pub type VHFHANDLE = *mut core::ffi::c_void;
pub type VHFOPERATIONHANDLE = *mut core::ffi::c_void;

#[repr(C)]
pub struct VHF_CONFIG {
    pub size: ULONG,
    pub vhf_client_context: PVOID,
    pub operation_context_size: ULONG,
    pub device_object: PDEVICE_OBJECT,
    pub vendor_id: USHORT,
    pub product_id: USHORT,
    pub version_number: USHORT,
    pub container_id: GUID,
    pub instance_id_length: USHORT,
    pub instance_id: PWSTR,
    pub report_descriptor_length: USHORT,
    pub report_descriptor: PUCHAR,
    pub evt_vhf_ready_for_next_read_report: Option<EVT_VHF_READY_FOR_NEXT_READ_REPORT>,
    pub evt_vhf_async_operation_get_feature: Option<EVT_VHF_ASYNC_OPERATION>,
    pub evt_vhf_async_operation_set_feature: Option<EVT_VHF_ASYNC_OPERATION>,
    pub evt_vhf_async_operation_write_report: Option<EVT_VHF_ASYNC_OPERATION>,
    pub evt_vhf_async_operation_get_input_report: Option<EVT_VHF_ASYNC_OPERATION>,
    pub evt_vhf_cleanup: Option<EVT_VHF_CLEANUP>,
    pub hardware_ids_length: USHORT,
    pub hardware_ids: PWSTR,
}

// Callback type aliases
pub type EVT_VHF_ASYNC_OPERATION = unsafe extern "C" fn(
    vhf_client_context: PVOID,
    vhf_operation_handle: VHFOPERATIONHANDLE,
    vhf_operation_context: PVOID,
    hid_transfer_packet: *mut HID_XFER_PACKET,
);

pub type EVT_VHF_READY_FOR_NEXT_READ_REPORT = unsafe extern "C" fn(
    vhf_client_context: PVOID,
);

pub type EVT_VHF_CLEANUP = unsafe extern "C" fn(
    vhf_client_context: PVOID,
);

extern "C" {
    pub fn VhfCreate(vhf_config: *mut VHF_CONFIG, vhf_handle: *mut VHFHANDLE) -> NTSTATUS;
    pub fn VhfStart(vhf_handle: VHFHANDLE) -> NTSTATUS;
    pub fn VhfReadReportSubmit(vhf_handle: VHFHANDLE, hid_transfer_packet: *mut HID_XFER_PACKET) -> NTSTATUS;
    pub fn VhfAsyncOperationComplete(vhf_operation_handle: VHFOPERATIONHANDLE, completion_status: NTSTATUS) -> NTSTATUS;
    pub fn VhfDelete(vhf_handle: VHFHANDLE, wait: BOOLEAN);
}
```

### 5.4 Multitouch Activation (Bluetooth)

```rust
/// Enable multitouch mode on BT Magic Trackpad
fn enable_multitouch_bt(io_target: &IoTarget) -> Result<()> {
    let report: [u8; 3] = [0xF1, 0x02, 0x01];
    send_hid_set_feature(io_target, 0xF1, &report)?;
    Ok(())
}
```

### 5.5 Recovery Mechanisms

```rust
/// Timer-based retry for transport failures (2-3 second interval)
/// Re-issues read requests after spurious completions
/// Uses lookaside list for HID read buffer allocation (ported from C)
```

---

## 6. INF Files

### 6.1 USB Driver INF (`amt-ptp-usb.inx`)

```ini
[Manufacturer]
%ManufacturerName% = Standard, NT$ARCH$

[Standard.NT$ARCH$]
; Magic Trackpad 2 USB
%DeviceName.MT2% = AmtPtpUsb_Install, USB\VID_05AC&PID_0265&MI_01
; Magic Trackpad 3 USB (PID TBD — placeholder)
; %DeviceName.MT3% = AmtPtpUsb_Install, USB\VID_05AC&PID_XXXX&MI_01

[AmtPtpUsb_Install.NT]
CopyFiles = AmtPtpUsb.CopyFiles

[AmtPtpUsb_Install.NT.Services]
AddService = AmtPtpUsb, 0x00000002, AmtPtpUsb_Service

[AmtPtpUsb_Service]
ServiceType    = 1                  ; SERVICE_KERNEL_DRIVER
StartType      = 3                  ; SERVICE_DEMAND_START
ErrorControl   = 1                  ; SERVICE_ERROR_NORMAL
ServiceBinary  = %13%\amt_ptp_usb.sys
```

### 6.2 Bluetooth Driver INF (`amt-ptp-bt.inx`)

```ini
[Standard.NT$ARCH$]
; Magic Trackpad 2 BT
%DeviceName.MT2BT% = AmtPtpBt_Install, HID\{00001124-0000-1000-8000-00805f9b34fb}_VID&0001004c_PID&0265&Col01
; Magic Trackpad 3 BT (PID TBD)

[AmtPtpBt_Install.NT]
CopyFiles = AmtPtpBt.CopyFiles

[AmtPtpBt_Install.NT.HW]
AddReg = AmtPtpBt_AddReg

[AmtPtpBt_AddReg]
; VHF as lower filter
HKR,,"LowerFilters",0x00010000,"vhf"

[AmtPtpBt_Install.NT.Services]
AddService = AmtPtpBt, 0x00000002, AmtPtpBt_Service

[AmtPtpBt_Service]
ServiceType    = 1                  ; SERVICE_KERNEL_DRIVER
StartType      = 3                  ; SERVICE_DEMAND_START
ErrorControl   = 1                  ; SERVICE_ERROR_NORMAL
ServiceBinary  = %13%\amt_ptp_bt.sys
```

---

## 7. Bug Fixes (from original C codebase)

| # | Bug | Fix | Status |
|---|-----|-----|--------|
| 1 | **Duplicate confidence check**: TouchMinor checked twice instead of TouchMinor AND TouchMajor | Check both: `touch_minor << 1 < 345 && touch_major << 1 < 345` | Fixed in Phase 3 |
| 2 | **Scan time cap too low**: Capped at 0xFF (25.5ms) but field is 16-bit | Cap at 0xFFFF (6.5 seconds, effectively uncapped for normal use) | Fixed in Phase 4 |
| 3 | **PTP_CONTACT struct inconsistency**: ContactID field size varies between modules | Standardize ContactID to match HID descriptor (use appropriate bit width) | Fixed in Phase 3 |
| 4 | **Missing defuzz**: Raw contact data can be jittery | Implement optional contact smoothing/defuzz filter in core | Deferred |
| 5 | **No emergency reset for BT**: Only USB had recovery mechanism | Add timer-based recovery for both transports | Phase 9 |
| 6 | **Scan time units wrong**: `/100` hardcoded instead of using QPC frequency. Gives ~10µs units, not 100µs. | Query `KeQueryPerformanceCounter` frequency, compute `ticks * 10000 / freq` | **Fixed in Phase 4.5** |
| 7 | **Selective reporting struct mismatch**: Extra `device_mode` byte in Rust struct doesn't exist in HID descriptor. SET_FEATURE always fails. | Remove `device_mode`, match C driver's 2-byte struct | **Fixed in Phase 4.5** |
| 8 | **Physical max = logical max in descriptor**: Windows sees ~76cm trackpad, breaking gesture DPI. | Add per-device `x_physical`/`y_physical` from C driver's WellspringMt2.h/T2.h | **Fixed in Phase 4.5** |

---

## 8. Settings App (`amt-ptp-settings`) — Phase 2

A native Windows app (not UWP) for trackpad configuration.

### Features
- Device discovery via HID device enumeration
- Battery status reading (report ID 0x90)
- Configuration:
  - Pressure qualification level
  - Single/multi-contact size qualification level
- Uses feature report 0x09 (REPORTID_UMAPP_CONF)

### Technology Options
- **Rust + windows-rs** — native Win32 GUI via Windows API
- **Rust + egui/iced** — cross-platform Rust GUI framework
- **C# WinUI 3** — modern Windows native (simpler, but mixed language)

Decision deferred to Phase 2.

---

## 9. Implementation Phases (Consolidated — 10 Phases)

Consolidated from the original 24-phase plan into 10 focused phases.
Each phase produces a substantial, testable deliverable.

---

### Phase 1: Workspace Scaffolding — **DONE** (commit `e5b622a`)

**Delivered:**
- `rust-toolchain.toml` pinned to `nightly-2025-02-10`
- Root `Cargo.toml` workspace with 4 members, KMDF 1.33 metadata, workspace deps
- `.cargo/config.toml` with `+crt-static` rustflag
- `crates/amt-ptp-core/` — `#![no_std]` rlib crate
- `crates/amt-ptp-usb/` — KMDF cdylib driver crate (wdk-sys features: usb, hid)
- `crates/amt-ptp-bt/` — KMDF cdylib driver crate (wdk-sys features: hid)
- `vhf-sys/` — placeholder FFI crate
- `build.rs` in each driver crate calling `wdk_build::configure_wdk_binary_build()`

---

### Phase 2: Build System & INX Files — **DONE** (commit `9772189`)

**Delivered:**
- `amt_ptp_usb.inx` — USB INF template matching 14 hardware IDs (13 T2 PIDs + MT2 0x0265), lower filter beneath mshidkmdf
- `amt_ptp_bt.inx` — BT filter INF matching MT2 BT/USB HID Col01, null-devices Col02/Col03
- `pkg/AmtPtpDevice.inx` — unified package INF shipping both .sys in single catalog
- `Makefile.toml` — cargo-make build automation (check, build, test, package, lint tasks)

---

### Phase 3: Core Library — **DONE** (commit `d2b242c`, 30 tests)

**Delivered** (consolidated original phases 3-10):
- `constants.rs` (188 lines) — Apple VID/PIDs, header/finger/button offsets, report IDs, thresholds
- `device.rs` (387 lines) — `TrackpadType` enum, `AxisParams`, `WellspringMsg`, `DeviceConfig`, `CONFIG_TABLE` (6 entries), `lookup_config()`
- `error.rs` (46 lines) — `Error` enum: `BufferTooShort`, `MalformedPayload`, `FingerIndexOutOfRange`
- `finger.rs` (350 lines) — `Finger` struct, `parse_type5_finger()` (13-bit signed extraction), `parse_legacy_finger()` (TYPE2-4), `parse_report()`, `transform_to_ptp()`, `is_tip_switch()`, `is_confident()`
- `ptp.rs` (284 lines) — `PtpContact` (9B), `PtpReport` (50B), 5 feature report structs, `as_bytes()` for zero-copy HID submission
- `hid.rs` (320 lines) — `DEFAULT_HQA_BLOB` (256B), `build_report_descriptor()` (3 TLCs, parameterized X/Y)

**Test Coverage:**
- Device config lookup (MT2, T2-13", unknown fallback, PTP logical max)
- TYPE5 coordinate extraction (origin, positive X, negative X, contact ID/orientation, touch fields)
- Finger transform (MT2 coordinates, negative clamping, tip switch, confidence)
- PTP report struct sizes (9, 50, 3, 257, 2, 3, 4 bytes)
- PTP report `as_bytes()` round-trip
- HID descriptor (builds for MT2, contains all report IDs, correct size range)
- HQA blob (length, first/last bytes)

---

### Phase 4: USB Driver — **DONE** (commit `958b09d`, 7 modules, ~1,400 lines)

**Delivered** (consolidated original phases 12-16):
- `lib.rs` (164 lines) — `DriverEntry` → `WdfDriverCreate` → `evt_driver_device_add` (filter setup, PnP/power callbacks, device creation with `DeviceContext`, queue initialization)
- `device.rs` (114 lines) — `DeviceContext` struct (USB handles, queues, config, PTP state, timing), `get_device_context()`, `DEVICE_CONTEXT_TYPE_INFO`
- `usb.rs` (325 lines) — `prepare_usb_hardware()` (USB device create, descriptor read, config lookup, interface select, pipe find, continuous reader), `set_wellspring_mode()` (read-modify-write control transfers), `evt_usb_interrupt_readers_failed()`
- `power.rs` (106 lines) — `evt_device_d0_entry()` (Wellspring on, record time, start reader), `evt_device_d0_exit()` (stop target, Wellspring off)
- `queue.rs` (165 lines) — HID IOCTL codes, `queue_initialize()` (default parallel + manual input, non-power-managed), `evt_io_internal_device_control()` dispatch, `dispatch_read_report()` (forward to manual queue)
- `hid.rs` (393 lines) — `HidDescriptor`/`HidDeviceAttributes`/`HidXferPacket` types, `get_hid_descriptor()`, `get_device_attributes()`, `get_report_descriptor()`, `get_feature()` (device caps + HQA), `set_feature()` (input mode → Wellspring, selective reporting)
- `input.rs` (147 lines) — `evt_usb_interrupt_pipe_read_complete()`: parse fingers → scan time → PTP report → dequeue & complete pending request

---

### Phase 4.5: USB Driver Code Review & Bug Fixes — **DONE**

**Goal:** Static code review of the USB driver against the C driver source and PTP spec.

**Bugs found and fixed:**

1. **Scan time frequency bug**: The scan time calculation divided the QPC tick delta
   by a hardcoded `100` instead of using the actual `KeQueryPerformanceCounter`
   frequency. On a typical 10MHz QPC this gave 10µs units instead of the
   PTP-required 100µs units (10× too fast). This would cause gesture velocity
   miscalculations in Windows.
   - **Fix**: Added `perf_freq` to `DeviceContext`, query frequency in `D0Entry`,
     compute `delta_100us = delta_ticks * 10_000 / frequency` in `input.rs`.
   - (The original C driver had the same `/100` pattern — this is a fix over both.)

2. **PtpSelectiveReportingReport struct mismatch**: The Rust struct had an extra
   `device_mode` field (3 bytes) that doesn't exist in the HID descriptor (which
   defines 2 bits + 6 padding = 1 data byte, 2 bytes total with report ID). This
   meant Windows SET_FEATURE for report 0x06 would always fail
   `STATUS_BUFFER_TOO_SMALL` — selective reporting could never work.
   - **Fix**: Removed `device_mode` field. Struct is now 2 bytes matching the
     C driver's `PTP_DEVICE_SELECTIVE_REPORT_MODE_REPORT` and the HID descriptor.

3. **Physical max values wrong in HID descriptor**: Both `get_hid_descriptor()` and
   `get_report_descriptor()` passed `ptp_x_logical_max()` as the physical max
   parameter. This told Windows the trackpad was ~76cm × 51cm (the coordinate span)
   instead of ~16cm × 11.5cm (the actual physical size), breaking DPI calculations
   and gesture scaling.
   - **Fix**: Added `x_physical` and `y_physical` fields to `DeviceConfig`, populated
     from the C driver's per-device HID headers (MT2: 1600×1149, T2: 1300×850).
     Driver now calls `build_report_descriptor(logical_x, logical_y, physical_x, physical_y)`.

**Test results:** 33 unit tests passing (up from 30).

---

### Phase 5: USB Driver On-Device Testing & Packaging — **MANUAL STEP (YOU)**

> **This phase requires a Windows machine with a Magic Trackpad 2 connected via USB.**
> It cannot be done in a CI/headless environment. Proceed to Phase 6 (VHF FFI bindings)
> in the meantime — Phase 5 is not blocking.
>
> **MUST-DO CHECKPOINT:** Phase 5 **must be completed before Phase 9** (BT driver
> testing). Both drivers share `amt-ptp-core`, so any issues found during USB
> on-device testing could affect the BT driver's input parsing. Ideally, do Phase 5
> right after Phase 6 or 7, while the USB code is still fresh in memory.

**Goal:** Build on Windows, install on test hardware, verify end-to-end functionality.

**Prerequisites:**
- Windows 11 22H2+ build machine with eWDK, VS2022, LLVM, Rust nightly
- Apple Magic Trackpad 2 (USB) or a T2 MacBook running Windows via Boot Camp
- Test-signing enabled (`bcdedit /set testsigning on`)

**Build steps:**
```powershell
cargo wdk build --release -p amt-ptp-usb
# Output: target/<arch>/release/amt_ptp_usb.sys + amt_ptp_usb.cat
```

**Testing checklist:**
- [ ] `cargo wdk build` produces `amt_ptp_usb.sys` + signed `.cat` without errors
- [ ] Driver installs on MT2 USB without errors (Device Manager → Update Driver)
- [ ] Device Manager shows "Apple USB Precision Touchpad Device" under HID
- [ ] Single finger move → cursor moves smoothly
- [ ] Two-finger scroll works (vertical and horizontal)
- [ ] Three-finger gestures work (task view, desktop switch)
- [ ] Pinch-to-zoom works
- [ ] Physical click works
- [ ] Tap-to-click works (if enabled in Windows Settings)
- [ ] Sleep → resume: cursor moves after wake, no BSOD
- [ ] Unplug → replug: device re-enumerates, cursor works
- [ ] No Driver Verifier violations (run with DV enabled)

**If issues are found:** Fix them and re-run the checklist. Document any additional
device-specific quirks discovered during testing.

---

### Phase 6: VHF-sys FFI Bindings

**Goal:** Rust FFI bindings for the Virtual HID Framework (`vhf.h`).

**Deliverables:**
- `vhf-sys/wrapper.h`, `build.rs` (bindgen + link `Vhfkm.lib`)
- `VHF_CONFIG`, `VHFHANDLE`, `VHFOPERATIONHANDLE`, callback types
- `VhfCreate`, `VhfStart`, `VhfReadReportSubmit`, `VhfAsyncOperationComplete`, `VhfDelete`

---

### Phase 7: BT Driver — Skeleton + VHF

**Goal:** KMDF filter driver that creates a VHF virtual PTP device.

**Deliverables:**
- `DriverEntry`, `EvtDriverDeviceAdd` (WdfFdoInitSetFilter)
- Self-managed I/O lifecycle (init, suspend, restart, cleanup)
- `VhfDevice` safe wrapper: create → start → submit → delete
- Virtual PTP device appears in Device Manager

---

### Phase 8: BT Driver — Transport & Input

**Goal:** Read raw BT HID data, transform to PTP, submit via VHF.

**Deliverables:**
- BT transport: `WdfIoTargetCreate`, read request chain, lookaside buffers
- Multitouch activation (feature report 0xF1)
- Read completion: parse BT header (4 bytes), `parse_report()`, `VhfReadReportSubmit()`
- VHF feature callbacks: get/set feature delegating to core

---

### Phase 9: BT Driver — Recovery, Testing & Packaging

**Goal:** Robust BT driver with error recovery. Installable package.

**Deliverables:**
- Timer-based retry, BT disconnect/reconnect handling
- INF verified, `cargo wdk build` produces `amt_ptp_bt.sys`
- Full BT testing checklist (same as USB + BT-specific: disconnect/reconnect, power cycle)

---

### Phase 10: MT3 Support, Settings App & Final Packaging

**Goal:** MT3 hardware support, settings app, unified distribution package.

**Deliverables:**
- MT3 protocol reverse-engineering, config entry, INF updates
- Settings app (Rust + windows-rs or egui): device discovery, battery status, config
- Unified INF + installer, production signing, documentation

---

## 10. Testing Strategy

### Unit Tests (user-mode, standard `cargo test`)
- `amt-ptp-core` is `#![no_std]` compatible but can be tested in user-mode
- Finger parsing: known byte sequences → expected coordinates
- PTP report generation: known inputs → expected report bytes
- Scan time calculation: edge cases (overflow, zero frequency)
- HID descriptor: validate descriptor structure and values

### Integration Tests
- Build and load drivers on test machine with MT2/MT3
- Verify device appears as PTP touchpad in Device Manager
- Test multitouch gestures (2-finger scroll, 3-finger swipe, pinch-to-zoom)
- Test button clicks (physical click + tap-to-click)
- Power management (sleep/resume cycles)
- BT reconnection after power cycle

### Validation Tools
- Windows HID Validator (built into HLK)
- `hidtool` / `hidparse` for descriptor validation
- Windows `tracelog` / ETW for driver tracing
- Driver Verifier (kernel-mode memory/IRQL checking)

---

## 11. Key Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| `windows-drivers-rs` doesn't expose needed APIs | Blocked | Manually extend `wdk-sys` bindings; worst case, use raw FFI to C headers |
| VHF doesn't work as BT filter pattern | Architecture change | Prototype VHF integration early (Phase 19); fallback to cleaned-up detour in Rust |
| MT3 protocol differs significantly from MT2 | Extra reverse-engineering | Start with MT2 only; MT3 is Phase 24 |
| Nightly Rust toolchain instability | Build breaks | Pin to known-good nightly version in `rust-toolchain.toml` |
| Driver signing for distribution | Can't distribute to users | Test-sign during development; EV cert for production |

---

## 12. Open Questions (to resolve during implementation)

1. **MT3 Product ID**: Need to identify via USB descriptor dump from user's MT3
2. **MT3 protocol differences**: May need USBPcap/Wireshark captures
3. **VHF + BT filter interaction**: Need to prototype whether VHF can be used from a filter driver position (vs function driver) — resolved in Phase 19
4. **HID descriptor ContactID width**: Should we use 4-bit (matching TYPE5 wire format) or 32-bit (matching original C driver)? PTP spec allows either — resolved in Phase 7
5. **Settings app technology**: Rust GUI vs C# WinUI — resolved in Phase 24
