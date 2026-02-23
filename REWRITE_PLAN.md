# Magic Trackpad 2/3 — Rust Driver Rewrite Plan

## Executive Summary

Rewrite the Windows Precision Touchpad driver for Apple Magic Trackpad 2 and 3 in Rust,
targeting USB and Bluetooth transports. Uses `windows-drivers-rs` for WDF bindings and
the Virtual HID Framework (VHF) to replace the fragile HIDCLASS detour hack.

---

## 1. Technology Stack

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Language | Rust | Safety, modern tooling, user preference |
| WDF Bindings | `windows-drivers-rs` (`wdk-sys` + `wdk`) | Official Microsoft crate, production-proven on Surface hardware |
| Driver Model | KMDF (both drivers) | VHF requires kernel-mode; consistency between USB and BT drivers |
| BT HID Virtualization | VHF (Virtual HID Framework) | Documented API, replaces fragile HIDCLASS detour hack |
| Build System | `cargo-wdk` + WDK 22H2 | Standard Rust toolchain with WDK integration |
| Min Windows Version | Windows 11 22H2 (build 22621) | Exact match with `windows-drivers-rs` default; zero WDK friction |
| Min WDF Version | KMDF 1.33 | Default for WDK 22H2, no manual `wdk-sys` patching needed |

### Build Prerequisites

- Windows 11 22H2+ (build machine)
- Enterprise WDK (eWDK) 22H2 or newer
- Visual Studio 2022 with "Desktop development with C++" workload
- LLVM 17.0.6 (for bindgen)
- Rust nightly toolchain (required for `#![no_std]` kernel drivers)
- `cargo-wdk` (`cargo install cargo-wdk`)

---

## 2. Project Structure

```
amt-ptp-rs/
├── Cargo.toml                    # Workspace root
├── README.md
├── crates/
│   ├── amt-ptp-core/             # Shared library crate
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            # Public API
│   │       ├── device.rs         # Device config tables (MT2/MT3)
│   │       ├── finger.rs         # TYPE5 finger parsing + coordinate transform
│   │       ├── ptp.rs            # PTP report generation (contacts, scan time)
│   │       ├── hid_descriptor.rs # HID report descriptor builder
│   │       ├── feature.rs        # Feature report handling (caps, HQA, mode)
│   │       └── config.rs         # Runtime configuration (sensitivity, thresholds)
│   │
│   ├── amt-ptp-usb/              # USB KMDF function driver
│   │   ├── Cargo.toml
│   │   ├── amt-ptp-usb.inx       # INF template
│   │   └── src/
│   │       ├── lib.rs            # DriverEntry, EvtDriverDeviceAdd
│   │       ├── device.rs         # USB device init, interface selection, Wellspring
│   │       ├── interrupt.rs      # USB interrupt transfer handling
│   │       ├── hid.rs            # HID minidriver IOCTL handling
│   │       └── queue.rs          # WDF queue setup + IOCTL dispatch
│   │
│   ├── amt-ptp-bt/               # Bluetooth KMDF filter driver + VHF
│   │   ├── Cargo.toml
│   │   ├── amt-ptp-bt.inx        # INF template
│   │   └── src/
│   │       ├── lib.rs            # DriverEntry, EvtDriverDeviceAdd
│   │       ├── device.rs         # Filter device init, VHF virtual device setup
│   │       ├── transport.rs      # BT HID transport read/write
│   │       ├── vhf.rs            # VHF lifecycle (create, start, submit, delete)
│   │       └── queue.rs          # WDF queue setup
│   │
│   └── amt-ptp-settings/         # Settings app (phase 2)
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
│
├── vhf-sys/                      # VHF FFI bindings (standalone crate)
│   ├── Cargo.toml
│   ├── build.rs                  # bindgen for vhf.h
│   ├── src/
│   │   └── lib.rs                # Raw VHF FFI types + functions
│   └── wrapper.h                 # #include <vhf.h>
│
└── tests/                        # Integration tests (user-mode simulation)
    ├── finger_parsing_tests.rs
    ├── ptp_report_tests.rs
    └── hid_descriptor_tests.rs
```

### Workspace Cargo.toml

```toml
[workspace]
members = [
    "crates/amt-ptp-core",
    "crates/amt-ptp-usb",
    "crates/amt-ptp-bt",
    "vhf-sys",
]
resolver = "2"

[workspace.dependencies]
wdk = "0.3"
wdk-sys = "0.3"
wdk-alloc = "0.3"
wdk-panic = "0.3"
amt-ptp-core = { path = "crates/amt-ptp-core" }
vhf-sys = { path = "vhf-sys" }
```

---

## 3. Crate Design: `amt-ptp-core`

The shared library containing all transport-independent logic.

### 3.1 Device Configuration (`device.rs`)

```rust
#[derive(Clone, Copy)]
pub struct TrackpadConfig {
    pub name: &'static str,
    pub vendor_id_usb: u16,      // 0x05AC
    pub vendor_id_bt: u16,       // 0x004C
    pub product_id: u16,         // 0x0265 (MT2), TBD (MT3)
    pub x_min: i16,
    pub x_max: i16,
    pub y_min: i16,
    pub y_max: i16,
    pub x_physical_mm: u16,      // 1600 = 160.0mm
    pub y_physical_mm: u16,      // 1149 = 114.9mm
    pub max_contacts: u8,        // 5
    pub finger_size: u8,         // 9 (TYPE5)
    pub usb_header_size: u8,     // 12
    pub bt_header_size: u8,      // 4
    pub button_offset: u8,       // 1
}

pub static MAGIC_TRACKPAD_2: TrackpadConfig = TrackpadConfig {
    name: "Magic Trackpad 2",
    vendor_id_usb: 0x05AC,
    vendor_id_bt: 0x004C,
    product_id: 0x0265,
    x_min: -3678,
    x_max: 3934,
    y_min: -2479,
    y_max: 2586,
    x_physical_mm: 1600,
    y_physical_mm: 1149,
    max_contacts: 5,
    finger_size: 9,
    usb_header_size: 12,
    bt_header_size: 4,
    button_offset: 1,
};

// MT3 config to be filled in after reverse engineering
pub static MAGIC_TRACKPAD_3: TrackpadConfig = TrackpadConfig {
    name: "Magic Trackpad 3",
    // ... TBD, likely same as MT2 with different PID and coordinate ranges
};
```

### 3.2 TYPE5 Finger Parsing (`finger.rs`)

Port of the C bit manipulation, with clear documentation of the wire format:

```rust
/// TYPE5 finger data: 9 bytes packed
/// Byte layout:
///   [0..1] bits 0-12:  absolute X (13-bit signed)
///   [0..3] bits 13-31: absolute Y (19-bit region, sign-extended)
///   [4]    touch major
///   [5]    touch minor
///   [6]    size (tool area)
///   [7]    pressure
///   [8]    bits 0-3: contact ID, bits 4-7: orientation
#[repr(C, packed)]
pub struct Type5Finger {
    pub data: [u8; 9],
}

impl Type5Finger {
    pub fn x_raw(&self) -> i16 {
        let raw = u16::from_le_bytes([self.data[0], self.data[1]]) & 0x1FFF;
        // Sign-extend 13-bit to 16-bit
        ((raw << 3) as i16) >> 3
    }

    pub fn y_raw(&self) -> i16 {
        let raw = u32::from_le_bytes([self.data[0], self.data[1], self.data[2], self.data[3]]);
        // Extract Y: shift left 6 to align, arithmetic right shift 19
        -(((raw << 6) as i32) >> 19) as i16
    }

    pub fn touch_major(&self) -> u8 { self.data[4] }
    pub fn touch_minor(&self) -> u8 { self.data[5] }
    pub fn size(&self) -> u8 { self.data[6] }
    pub fn pressure(&self) -> u8 { self.data[7] }
    pub fn contact_id(&self) -> u8 { self.data[8] & 0x0F }
    pub fn orientation(&self) -> u8 { (self.data[8] >> 4) & 0x0F }
}
```

### 3.3 PTP Report Generation (`ptp.rs`)

```rust
/// Windows Precision Touchpad contact
#[repr(C, packed)]
pub struct PtpContact {
    pub flags: u8,          // bit 0: Confidence, bit 1: TipSwitch
    pub contact_id: u32,
    pub x: u16,
    pub y: u16,
}

/// Full PTP input report
#[repr(C, packed)]
pub struct PtpReport {
    pub report_id: u8,      // 0x05
    pub contacts: [PtpContact; 5],
    pub scan_time: u16,     // 100us units, max 0xFFFF (BUG FIX: was 0xFF)
    pub contact_count: u8,
    pub is_button_clicked: u8,
}

/// Transform raw Apple finger data into PTP contacts
pub fn transform_fingers(
    raw_data: &[u8],
    config: &TrackpadConfig,
    header_size: u8,        // USB vs BT header size
    report: &mut PtpReport,
) -> u8 {
    let finger_data = &raw_data[header_size as usize..];
    let num_fingers = finger_data.len() / config.finger_size as usize;
    let count = num_fingers.min(config.max_contacts as usize);

    for i in 0..count {
        let offset = i * config.finger_size as usize;
        let finger = Type5Finger { data: finger_data[offset..offset+9].try_into().unwrap() };

        let x_raw = finger.x_raw();
        let y_raw = finger.y_raw();

        // Translate to PTP coordinate space (0-based, clamped)
        let x = (x_raw - config.x_min).max(0) as u16;
        let y = (y_raw - config.y_min).max(0) as u16;

        // BUG FIX: check both touch_major AND touch_minor for confidence
        let tip_switch = (finger.touch_major() << 1) > 0;
        let confidence = (finger.touch_minor() << 1) < 345
                      && (finger.touch_major() << 1) < 345;

        report.contacts[i] = PtpContact {
            flags: (confidence as u8) | ((tip_switch as u8) << 1),
            contact_id: finger.contact_id() as u32,
            x,
            y,
        };
    }

    count as u8
}
```

### 3.4 HID Report Descriptor (`hid_descriptor.rs`)

Builder pattern for constructing the PTP HID report descriptor at compile time:

```rust
/// Generate the complete PTP HID report descriptor for a given trackpad config.
/// This replaces the static byte arrays from the C codebase.
pub fn build_ptp_descriptor(config: &TrackpadConfig) -> &'static [u8] {
    // ... builds descriptor with:
    // - Digitizer/TouchPad usage page (0x0D, usage 0x05)
    // - 5 finger collections (Confidence, TipSwitch, ContactID, X, Y)
    // - Scan Time (16-bit, 100us units)
    // - Contact Count
    // - Button (1 bit + 7 padding)
    // - Device Caps feature report (max contacts, button type)
    // - HQA certification feature report (256 bytes)
    // - Configuration TLC (input mode, function switch)
    //
    // Logical/physical maximums derived from config:
    //   X logical max = config.x_max - config.x_min
    //   Y logical max = config.y_max - config.y_min
    //   X physical max = config.x_physical_mm
    //   Y physical max = config.y_physical_mm
}
```

### 3.5 Feature Reports (`feature.rs`)

```rust
/// PTP Device Capabilities feature report
#[repr(C, packed)]
pub struct DeviceCapsReport {
    pub report_id: u8,          // 0x07
    pub max_contact_points: u8, // 5
    pub button_type: u8,        // 0 = clickpad
}

/// PTP HQA certification report
#[repr(C, packed)]
pub struct HqaCertReport {
    pub report_id: u8,          // 0x08
    pub blob: [u8; 256],        // Hardcoded certification blob
}

/// Input mode feature report
#[repr(C, packed)]
pub struct InputModeReport {
    pub report_id: u8,          // 0x04
    pub mode: u8,               // 0=mouse, 3=PTP
}

/// Function switch feature report
#[repr(C, packed)]
pub struct FunctionSwitchReport {
    pub report_id: u8,          // 0x06
    pub switch_state: u8,       // bit 0: button, bit 1: surface
}
```

### 3.6 Scan Time (`ptp.rs`)

```rust
/// BUG FIX: Cap at 0xFFFF (16-bit) instead of original 0xFF (8-bit)
pub fn calculate_scan_time(
    current_counter: u64,
    last_counter: u64,
    frequency: u64,
) -> u16 {
    if frequency == 0 || last_counter == 0 {
        return 0;
    }
    let delta = current_counter.saturating_sub(last_counter);
    // Convert to 100us units: delta * 10000 / frequency
    let time_100us = (delta * 10_000) / frequency;
    time_100us.min(0xFFFF) as u16
}
```

---

## 4. USB Driver: `amt-ptp-usb`

KMDF function driver for Magic Trackpad 2/3 connected via USB.

### 4.1 Driver Lifecycle

```
DriverEntry
  → EvtDriverDeviceAdd
    → WdfDeviceCreate (with PnP/Power callbacks)
    → WdfIoQueueCreate (default queue for HID IOCTLs)
    → WdfIoQueueCreate (manual queue for pending read requests)

EvtDevicePrepareHardware
  → Select USB configuration + interface
  → Find interrupt IN pipe
  → Store device config (MT2 or MT3 based on PID)

EvtDeviceD0Entry
  → Enable Wellspring mode (USB control transfer)
  → Configure continuous reader on interrupt pipe
  → Start scan time counter

EvtDeviceD0Exit
  → Disable Wellspring mode
  → Stop continuous reader

EvtIoInternalDeviceControl
  → Route HID IOCTLs:
    IOCTL_HID_GET_DEVICE_DESCRIPTOR      → serve HID_DESCRIPTOR
    IOCTL_HID_GET_REPORT_DESCRIPTOR      → serve PTP report descriptor
    IOCTL_HID_GET_DEVICE_ATTRIBUTES      → serve HID_DEVICE_ATTRIBUTES
    IOCTL_HID_READ_REPORT                → queue in manual queue
    IOCTL_HID_GET_FEATURE                → serve feature reports
    IOCTL_HID_SET_FEATURE                → handle input mode, config
    IOCTL_HID_GET_STRING                  → serve device strings

EvtUsbInterruptPipeReadComplete (continuous reader callback)
  → Parse USB header (12 bytes)
  → Extract button state from offset 1
  → Call amt_ptp_core::transform_fingers()
  → Calculate scan time
  → Complete pending read request from manual queue
```

### 4.2 Wellspring Mode Switching

```rust
/// Enable raw multitouch data from the trackpad
fn wellspring_mode_set(device: &UsbDevice, config: &TrackpadConfig, enable: bool) -> Result<()> {
    // 1. Read current mode via GET_REPORT class request
    let mut mode_buf = vec![0u8; config.um_size];
    usb_control_transfer(
        device,
        BmRequestDeviceToHost | BmRequestClass | BmRequestToInterface,
        HID_GET_REPORT,
        config.um_req_val,
        config.um_req_idx,
        &mut mode_buf,
    )?;

    // 2. Modify mode byte
    mode_buf[config.um_switch_idx] = if enable { config.um_switch_on } else { config.um_switch_off };

    // 3. Write back via SET_REPORT class request
    usb_control_transfer(
        device,
        BmRequestHostToDevice | BmRequestClass | BmRequestToInterface,
        HID_SET_REPORT,
        config.um_req_val,
        config.um_req_idx,
        &mode_buf,
    )?;

    Ok(())
}
```

### 4.3 Emergency Reset

```rust
/// Recovery mechanism: toggle Wellspring off and back on
fn emergency_reset(device: &UsbDevice, config: &TrackpadConfig) -> Result<()> {
    wellspring_mode_set(device, config, false)?;
    wellspring_mode_set(device, config, true)?;
    Ok(())
}
```

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

| # | Bug | Fix |
|---|-----|-----|
| 1 | **Duplicate confidence check**: TouchMinor checked twice instead of TouchMinor AND TouchMajor | Check both: `touch_minor << 1 < 345 && touch_major << 1 < 345` |
| 2 | **Scan time cap too low**: Capped at 0xFF (25.5ms) but field is 16-bit | Cap at 0xFFFF (6.5 seconds, effectively uncapped for normal use) |
| 3 | **PTP_CONTACT struct inconsistency**: ContactID field size varies between modules | Standardize ContactID to match HID descriptor (use appropriate bit width) |
| 4 | **Missing defuzz**: Raw contact data can be jittery | Implement optional contact smoothing/defuzz filter in core |
| 5 | **No emergency reset for BT**: Only USB had recovery mechanism | Add timer-based recovery for both transports |

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

## 9. Implementation Phases (24 Phases)

Below is the full 24-phase implementation plan. Each phase produces a concrete,
testable deliverable. Phases are sequential — each builds on the previous.

---

### Phase 1: Rust Toolchain & Workspace Scaffolding

**Goal:** Empty Cargo workspace that compiles with the WDK toolchain.

**Deliverables:**
- `rust-toolchain.toml` pinned to known-good nightly
- Root `Cargo.toml` workspace with member stubs
- `.cargo/config.toml` with WDK linker settings
- `crates/amt-ptp-core/` — empty `#![no_std]` lib crate
- `crates/amt-ptp-usb/` — empty KMDF driver crate stub
- `crates/amt-ptp-bt/` — empty KMDF driver crate stub
- `vhf-sys/` — empty FFI crate stub
- Verify `cargo build` succeeds with WDK environment active

**Key decisions:**
- Pin nightly date in `rust-toolchain.toml` for reproducibility
- Configure `[package.metadata.wdk]` for KMDF 1.33 in each driver crate
- Set up `wdk-alloc` global allocator and `wdk-panic` handler in driver crates

---

### Phase 2: Build System & CI Configuration

**Goal:** Reproducible builds with `cargo-wdk`, INF stamping, and catalog generation.

**Deliverables:**
- `cargo-wdk` integration verified (build → stampinf → inf2cat → signtool)
- Test-signing certificate generated for development
- Build script (`build.rs`) in each driver crate configuring `wdk-build`
- `.gitignore` for build artifacts (`.sys`, `.dll`, `.cat`, `.cer`)
- Document build steps in `README.md`

**Verification:** `cargo wdk build` produces a `.sys` file + signed `.cat` for each driver crate (even if the driver does nothing yet).

---

### Phase 3: Core — Device Configuration Types

**Goal:** Type-safe device configuration table in `amt-ptp-core`.

**Deliverables:**
- `crates/amt-ptp-core/src/device.rs`:
  - `TrackpadConfig` struct with all device parameters (VID, PID, coordinate ranges, header sizes, Wellspring params, physical dimensions)
  - `WellspringConfig` sub-struct for USB mode switching params
  - `static MAGIC_TRACKPAD_2: TrackpadConfig` fully populated
  - `static MAGIC_TRACKPAD_3: TrackpadConfig` placeholder (same as MT2, PID TBD)
  - `fn config_for_usb_pid(pid: u16) -> Option<&TrackpadConfig>`
  - `fn config_for_bt_pid(pid: u16) -> Option<&TrackpadConfig>`
- `crates/amt-ptp-core/src/lib.rs`: module declarations, `#![no_std]`

**Verification:** `cargo build` for core crate succeeds. Config lookup functions work in unit tests.

---

### Phase 4: Core — TYPE5 Finger Parsing

**Goal:** Bit-accurate parsing of Apple's 9-byte TYPE5 finger format.

**Deliverables:**
- `crates/amt-ptp-core/src/finger.rs`:
  - `Type5Finger` — `#[repr(C, packed)]` 9-byte struct
  - `x_raw(&self) -> i16` — 13-bit sign-extended X extraction
  - `y_raw(&self) -> i16` — Y extraction with sign extension and negation
  - `touch_major()`, `touch_minor()`, `size()`, `pressure()` — direct byte reads
  - `contact_id() -> u8` — low 4 bits of byte 8
  - `orientation() -> u8` — high 4 bits of byte 8
  - `is_valid(&self) -> bool` — sanity check (non-zero touch area)
- Unit tests with known byte sequences from the original C driver:
  - Zero finger (all zeros)
  - Positive X/Y coordinates
  - Negative X/Y coordinates (sign extension edge cases)
  - Maximum values (13-bit X = ±4095)
  - Contact ID 0-15

**Verification:** All unit tests pass. Byte-level compatibility with C implementation confirmed.

---

### Phase 5: Core — Coordinate Transformation & Contact Mapping

**Goal:** Transform raw Apple coordinates to PTP coordinate space.

**Deliverables:**
- `crates/amt-ptp-core/src/transform.rs`:
  - `fn transform_coordinate(raw: i16, min: i16) -> u16` — subtract min, clamp to >= 0
  - `fn compute_tip_switch(touch_major: u8) -> bool` — `(touch_major << 1) > 0`
  - `fn compute_confidence(touch_major: u8, touch_minor: u8) -> bool` — **BUG FIX**: check BOTH axes `< 345`
  - `fn parse_finger_to_contact(finger: &Type5Finger, config: &TrackpadConfig) -> PtpContact`
  - `fn parse_report_buffer(buf: &[u8], header_size: u8, config: &TrackpadConfig, contacts: &mut [PtpContact; 5]) -> (u8, bool)` — returns (contact_count, button_clicked)
- Button state extraction from header byte at `config.button_offset`

**Verification:** Unit tests with crafted buffers. Edge cases: 0 fingers, 1 finger, 5 fingers, >5 fingers (clamp), button pressed/released.

---

### Phase 6: Core — PTP Report Structures & Generation

**Goal:** Complete PTP report types matching the Windows PTP specification.

**Deliverables:**
- `crates/amt-ptp-core/src/ptp.rs`:
  - `PtpContact` — `#[repr(C, packed)]` (flags, contact_id, x, y)
  - `PtpReport` — `#[repr(C, packed)]` (report_id=0x05, contacts[5], scan_time, contact_count, is_button_clicked)
  - `fn build_ptp_report(contacts: &[PtpContact; 5], count: u8, scan_time: u16, button: bool) -> PtpReport`
  - Scan time calculation:
    - `fn calculate_scan_time(current: u64, last: u64, frequency: u64) -> u16`
    - **BUG FIX:** cap at `0xFFFF` not `0xFF`
  - `const REPORTID_MULTITOUCH: u8 = 0x05`
  - Verify struct sizes match expected byte counts with `static_assert`

**Verification:** `core::mem::size_of::<PtpReport>()` == expected. Round-trip tests: build report → inspect bytes → match expected layout.

---

### Phase 7: Core — HID Report Descriptor Builder

**Goal:** Generate the PTP HID report descriptor programmatically, replacing static C byte arrays.

**Deliverables:**
- `crates/amt-ptp-core/src/hid_descriptor.rs`:
  - Builder macros/functions for HID descriptor items:
    - `usage_page()`, `usage()`, `collection()`, `end_collection()`
    - `logical_minimum()`, `logical_maximum()`, `physical_minimum()`, `physical_maximum()`
    - `report_size()`, `report_count()`, `report_id()`
    - `input()`, `feature()`, `unit()`, `unit_exponent()`
  - `fn build_ptp_report_descriptor(config: &TrackpadConfig) -> &'static [u8]` or `const` array:
    - Digitizer/TouchPad TLC (usage page 0x0D, usage 0x05)
    - 5 finger collections (Confidence, TipSwitch, ContactID, X, Y)
    - Scan Time (16-bit, unit 100us)
    - Contact Count
    - Button (1 bit + 7 padding)
    - Maximum Contact Count feature report (report ID 0x07)
    - HQA certification feature report (report ID 0x08, 256 bytes)
    - Configuration TLC (input mode report ID 0x04, function switch report ID 0x06)
  - **BUG FIX:** ContactID width consistent (4-bit matching TYPE5 wire format)
- Byte-for-byte comparison test against original C descriptor (from `MagicTrackpad2.h`)

**Verification:** Generated descriptor matches expected bytes. Parseable by a HID descriptor parser tool.

---

### Phase 8: Core — Feature Report Handling

**Goal:** All PTP feature report types and their serialization/deserialization.

**Deliverables:**
- `crates/amt-ptp-core/src/feature.rs`:
  - `DeviceCapsReport` — report ID 0x07 (max_contacts=5, button_type=0 clickpad)
  - `HqaCertReport` — report ID 0x08 (256-byte hardcoded certification blob)
  - `InputModeReport` — report ID 0x04 (mode: 0=mouse, 3=PTP)
  - `FunctionSwitchReport` — report ID 0x06 (button/surface enable bits)
  - `UserModeConfigReport` — report ID 0x09 (pressure/size thresholds for settings app)
  - `fn handle_get_feature(report_id: u8, state: &DriverState, buf: &mut [u8]) -> Result<usize>`
  - `fn handle_set_feature(report_id: u8, state: &mut DriverState, buf: &[u8]) -> Result<()>`
  - `DriverState` struct holding runtime state (input_mode, function_switch, config thresholds)
- `HID_DESCRIPTOR` struct for `IOCTL_HID_GET_DEVICE_DESCRIPTOR`
- `HID_DEVICE_ATTRIBUTES` population helper

**Verification:** Unit tests for each report: serialize → check bytes → deserialize → check fields.

---

### Phase 9: Core — Scan Time, Utilities & Error Types

**Goal:** Remaining shared utilities and a clean error type.

**Deliverables:**
- `crates/amt-ptp-core/src/time.rs`:
  - `ScanTimeTracker` struct (last_counter, frequency)
  - `fn update(&mut self, current_counter: u64) -> u16` — returns scan time in 100us units
  - Handles first-call (no previous timestamp) gracefully
- `crates/amt-ptp-core/src/error.rs`:
  - `enum PtpError { InvalidBuffer, UnsupportedDevice, FeatureNotSupported, ... }`
  - Conversion to `NTSTATUS` for driver callers
- `crates/amt-ptp-core/src/constants.rs`:
  - All report IDs as named constants
  - Apple vendor IDs, product IDs
  - HQA certification blob as `static` array
  - PTP max contact points
- Clean up `lib.rs` public API — re-exports for driver crates

**Verification:** Scan time tracker unit tests with simulated counter sequences. Error conversion tests.

---

### Phase 10: Core — Comprehensive Unit Test Suite

**Goal:** Full test coverage for `amt-ptp-core` before starting driver work.

**Deliverables:**
- `crates/amt-ptp-core/tests/` (integration tests, run in user-mode):
  - `finger_parsing.rs` — exhaustive TYPE5 byte parsing tests
  - `coordinate_transform.rs` — boundary conditions, clamping, sign extension
  - `ptp_report.rs` — report generation, struct layout, byte-level verification
  - `hid_descriptor.rs` — descriptor validity, comparison with C original
  - `feature_reports.rs` — all feature report types serialize/deserialize correctly
  - `scan_time.rs` — overflow, zero frequency, large gaps, normal operation
  - `end_to_end.rs` — raw Apple USB/BT packet → complete PTP report (full pipeline)
- Test with captured real-world packets from the original driver (if available) or synthetic equivalents
- `cargo test` passes with 100% of `amt-ptp-core` public API covered

**Verification:** `cargo test` — all pass. This is the quality gate before Phase 11.

---

### Phase 11: VHF-sys — FFI Bindings Crate

**Goal:** Rust FFI bindings for the Virtual HID Framework (`vhf.h`).

**Deliverables:**
- `vhf-sys/wrapper.h`: `#include <vhf.h>`
- `vhf-sys/build.rs`: bindgen configuration targeting `vhf.h`, link `Vhfkm.lib`
- `vhf-sys/src/lib.rs`:
  - `VHFHANDLE`, `VHFOPERATIONHANDLE` type aliases
  - `VHF_CONFIG` struct (`#[repr(C)]`)
  - Callback type aliases: `EVT_VHF_ASYNC_OPERATION`, `EVT_VHF_READY_FOR_NEXT_READ_REPORT`, `EVT_VHF_CLEANUP`
  - Extern functions: `VhfCreate`, `VhfStart`, `VhfReadReportSubmit`, `VhfAsyncOperationComplete`, `VhfDelete`
  - `VHF_CONFIG_INIT` as a safe Rust helper function
  - `HID_XFER_PACKET` struct (if not already in `wdk-sys`)
- If bindgen can process `vhf.h` cleanly, use auto-generated bindings; otherwise hand-write them matching the Microsoft documentation exactly

**Verification:** Crate compiles and links against `Vhfkm.lib`. Type sizes match C equivalents (checked via `static_assert` or build-time test).

---

### Phase 12: USB Driver — KMDF Skeleton

**Goal:** Minimal KMDF driver that loads, creates a device object, and unloads cleanly.

**Deliverables:**
- `crates/amt-ptp-usb/src/lib.rs`:
  - `#![no_std]`, `#![no_main]`
  - `extern crate wdk_alloc` + `extern crate wdk_panic`
  - `#[export_name = "DriverEntry"]` function
  - `WdfDriverCreate` with `EvtDriverDeviceAdd` callback
- `crates/amt-ptp-usb/src/device.rs`:
  - `EvtDriverDeviceAdd`:
    - `WdfDeviceCreate` with PnP/power callback registration
    - Device context allocation (`DEVICE_CONTEXT` struct with fields for USB handles, config, state)
    - Two WDF queues: default (parallel, for HID IOCTLs) + manual (for pending read requests)
  - `EvtDeviceCleanupCallback` stub
- `crates/amt-ptp-usb/Cargo.toml`: dependencies on `wdk`, `wdk-sys`, `wdk-alloc`, `wdk-panic`, `amt-ptp-core`

**Verification:** Driver builds to `.sys`. Can be installed on a test machine (does nothing, but loads/unloads without BSOD). Check with `sc query` or Device Manager.

---

### Phase 13: USB Driver — Device Initialization & USB Interface Selection

**Goal:** Driver discovers USB device, selects correct interface and interrupt pipe.

**Deliverables:**
- `crates/amt-ptp-usb/src/device.rs` additions:
  - `EvtDevicePrepareHardware`:
    - Create WDFUSBDEVICE via `WdfUsbTargetDeviceCreateWithParameters`
    - `WdfUsbTargetDeviceGetDeviceDescriptor` — read VID/PID
    - Match PID against `amt_ptp_core::config_for_usb_pid()` to get `TrackpadConfig`
    - `WdfUsbTargetDeviceSelectConfig` — select first configuration
    - Iterate interfaces via `WdfUsbTargetDeviceGetInterface`, find MI_01
    - `WdfUsbInterfaceSelectSetting` — select alternate setting
    - Find interrupt IN pipe via `WdfUsbInterfaceGetConfiguredPipe`
    - Store USB device handle, pipe handle, and config in device context
  - `EvtDeviceReleaseHardware`:
    - Release USB resources
  - `DEVICE_CONTEXT` struct with all necessary fields

**Verification:** Install driver on MT2 USB. Debug traces show correct PID detected, interface selected, interrupt pipe found. Device Manager shows no errors.

---

### Phase 14: USB Driver — Wellspring Mode Switching

**Goal:** Enable/disable Apple multitouch mode via USB control transfers.

**Deliverables:**
- `crates/amt-ptp-usb/src/device.rs` additions:
  - `fn wellspring_set_mode(device_context: &DeviceContext, enable: bool) -> NTSTATUS`:
    - Build `WDF_USB_CONTROL_SETUP_PACKET` for GET_REPORT (class, device-to-host)
    - `WdfUsbTargetDeviceSendControlTransferSynchronously` to read current mode
    - Modify mode byte at `config.um_switch_idx`
    - Build SET_REPORT control setup packet (class, host-to-device)
    - Send modified buffer back
  - `EvtDeviceD0Entry`: call `wellspring_set_mode(true)`
  - `EvtDeviceD0Exit`: call `wellspring_set_mode(false)`
  - Emergency reset function: toggle off → on

**Verification:** After D0 entry, trackpad LED behavior changes (indicates mode switch). USB analyzer (USBPcap/Wireshark) shows correct control transfer sequence matching the original C driver.

---

### Phase 15: USB Driver — Interrupt Pipe & Touch Input Processing

**Goal:** Receive raw touch data from USB interrupt pipe and produce PTP reports.

**Deliverables:**
- `crates/amt-ptp-usb/src/interrupt.rs`:
  - Configure WDF continuous reader on interrupt IN pipe:
    - `WDF_USB_CONTINUOUS_READER_CONFIG_INIT` with completion callback
    - `WdfUsbTargetPipeConfigContinuousReader`
  - `EvtUsbTargetPipeReadComplete` callback:
    - Read raw buffer from `WdfMemoryGetBuffer`
    - Validate buffer size (>= header_size + at least 0 fingers)
    - Extract button state from `buffer[config.button_offset]`
    - Calculate number of fingers: `(data_len - header_size) / finger_size`
    - Call `amt_ptp_core::parse_report_buffer()` to transform fingers
    - Call `amt_ptp_core::build_ptp_report()` with contacts + scan time
    - Dequeue pending read request from manual queue
    - Copy PTP report to request output buffer
    - Complete the request with `WdfRequestComplete`
  - `EvtUsbTargetPipeReadersFailed` callback:
    - Log error, attempt emergency reset
    - Return TRUE to retry

**Verification:** Connect MT2 via USB. Touch the trackpad. Debug traces show finger data being parsed. If a HID client is reading (even a test tool), PTP reports are delivered.

---

### Phase 16: USB Driver — HID Minidriver IOCTL Dispatch

**Goal:** Complete HID minidriver interface — Windows sees a Precision Touchpad.

**Deliverables:**
- `crates/amt-ptp-usb/src/hid.rs`:
  - `fn dispatch_hid_ioctl(queue: WDFQUEUE, request: WDFREQUEST, ioctl: ULONG, ...)`:
    - `IOCTL_HID_GET_DEVICE_DESCRIPTOR` → return `HID_DESCRIPTOR` struct
    - `IOCTL_HID_GET_REPORT_DESCRIPTOR` → return PTP descriptor from `amt_ptp_core::build_ptp_report_descriptor()`
    - `IOCTL_HID_GET_DEVICE_ATTRIBUTES` → return `HID_DEVICE_ATTRIBUTES` (VID, PID, version)
    - `IOCTL_HID_READ_REPORT` → queue in manual queue (completed by interrupt handler)
    - `IOCTL_HID_GET_FEATURE` → delegate to `amt_ptp_core::handle_get_feature()`
    - `IOCTL_HID_SET_FEATURE` → delegate to `amt_ptp_core::handle_set_feature()`
    - `IOCTL_HID_GET_STRING` → return manufacturer/product/serial strings
    - All others → `STATUS_NOT_SUPPORTED`
- `crates/amt-ptp-usb/src/queue.rs`:
  - Default queue `EvtIoInternalDeviceControl` routes to `dispatch_hid_ioctl`
  - Manual queue for read report requests

**Verification:** Device appears as "HID-compliant touch pad" in Device Manager. Windows Settings → Touchpad shows the device. Basic touch input works.

---

### Phase 17: USB Driver — INF, Build, Package & Initial Testing

**Goal:** Complete, installable USB driver package. First end-to-end test.

**Deliverables:**
- `crates/amt-ptp-usb/amt-ptp-usb.inx`:
  - Hardware ID: `USB\VID_05AC&PID_0265&MI_01`
  - Service configuration (KMDF kernel driver, demand start)
  - KMDF coinstaller directives
  - Device description strings
- Build verification:
  - `cargo wdk build` produces: `amt_ptp_usb.sys`, `amt-ptp-usb.inf`, `amt-ptp-usb.cat`
  - Test-signed with development certificate
- Testing checklist:
  - [ ] Driver installs on MT2 USB without errors
  - [ ] Device Manager shows "Precision Touchpad" device
  - [ ] Single finger move → cursor moves
  - [ ] Two-finger scroll works
  - [ ] Three-finger gestures work (swipe, task view)
  - [ ] Pinch-to-zoom works
  - [ ] Physical click works
  - [ ] Tap-to-click works
  - [ ] Sleep/resume: touchpad recovers
  - [ ] Unplug/replug: device re-enumerates correctly
- Bug fixes for any issues found during testing

**Verification:** All checklist items pass. USB driver is feature-complete for MT2.

---

### Phase 18: BT Driver — KMDF Filter Driver Skeleton

**Goal:** Minimal KMDF filter driver that loads in the BT HID stack without disrupting it.

**Deliverables:**
- `crates/amt-ptp-bt/src/lib.rs`:
  - `#![no_std]`, `#![no_main]`
  - `DriverEntry` → `WdfDriverCreate`
- `crates/amt-ptp-bt/src/device.rs`:
  - `EvtDriverDeviceAdd`:
    - `WdfFdoInitSetFilter` — mark as filter driver
    - `WdfDeviceCreate` with PnP/power/self-managed-IO callbacks
    - `DEVICE_CONTEXT` struct (VHF handle, IO target for BT transport, config, state, buffers)
    - Default WDF queue (forward-all for now)
  - Self-managed I/O stubs (init, suspend, restart, cleanup)

**Verification:** Driver loads as filter in BT HID stack. Original BT mouse functionality still works (filter is pass-through). No BSOD.

---

### Phase 19: BT Driver — VHF Virtual Device Creation & Lifecycle

**Goal:** Create a virtual PTP device via VHF that Windows recognizes as a touchpad.

**Deliverables:**
- `crates/amt-ptp-bt/src/vhf.rs`:
  - Safe wrapper around `vhf-sys` raw FFI:
    - `struct VhfDevice { handle: VHFHANDLE }`
    - `fn create(wdm_device: PDEVICE_OBJECT, descriptor: &[u8], config: &TrackpadConfig, callbacks: VhfCallbacks) -> Result<Self>`
    - `fn start(&self) -> Result<()>`
    - `fn submit_report(&self, report: &PtpReport) -> Result<()>` — wraps `VhfReadReportSubmit`
    - `fn complete_async_op(handle: VHFOPERATIONHANDLE, status: NTSTATUS)`
    - `fn delete(self)` — consumes self, calls `VhfDelete(wait=TRUE)`
  - `VhfCallbacks` struct with function pointers for get/set feature
- `crates/amt-ptp-bt/src/device.rs` additions:
  - `EvtDeviceSelfManagedIoInit`:
    - Get WDM device object via `WdfDeviceWdmGetDeviceObject`
    - Get `TrackpadConfig` for MT2 BT
    - Build PTP report descriptor via `amt_ptp_core`
    - Call `VhfDevice::create()` + `start()`
    - Store VHF device in context
  - `EvtDeviceSelfManagedIoCleanup`:
    - Call `VhfDevice::delete()`

**Verification:** When driver loads on BT MT2, a second HID device appears in Device Manager — the virtual PTP touchpad created by VHF. Windows Settings → Touchpad sees it.

---

### Phase 20: BT Driver — Bluetooth HID Transport Communication

**Goal:** Read raw HID reports from the real BT device and send feature reports.

**Deliverables:**
- `crates/amt-ptp-bt/src/transport.rs`:
  - `struct BtTransport { io_target: WDFIOTARGET }`
  - Setup:
    - Create self-managed IO target (`WdfIoTargetCreate`)
    - Open lower device's WDM device object
    - `WdfIoTargetOpen` with `WdfIoTargetOpenUseExistingDevice`
  - Read path:
    - `fn issue_read_request(&self, context: &DeviceContext) -> Result<()>`:
      - Allocate buffer from lookaside list
      - Build `IOCTL_HID_READ_REPORT` IRP
      - `WdfIoTargetSendInternalIoctlAsynchronously` with completion callback
    - Completion callback registered to process incoming data
  - Write path:
    - `fn send_set_feature(&self, report_id: u8, data: &[u8]) -> Result<()>`:
      - Build `IOCTL_HID_SET_FEATURE` with `HID_XFER_PACKET`
      - `WdfIoTargetSendInternalIoctlSynchronously`
  - Buffer management:
    - WDF lookaside list for read buffers (`WdfLookasideListCreate`)
    - Proper cleanup on completion

**Verification:** Debug traces show raw BT HID reports being received from the trackpad. Feature report (multitouch enable) sent successfully.

---

### Phase 21: BT Driver — Input Processing & VHF Report Submission

**Goal:** Complete data pipeline: BT raw data → transform → VHF virtual PTP device.

**Deliverables:**
- `crates/amt-ptp-bt/src/transport.rs` — read completion callback:
  - Validate incoming buffer (check report ID, minimum size)
  - Parse BT header (4 bytes for TYPE5 BT)
  - Extract button state
  - Call `amt_ptp_core::parse_report_buffer()` with BT header size
  - Call `amt_ptp_core::build_ptp_report()`
  - Update scan time via `ScanTimeTracker`
  - Call `vhf_device.submit_report(&ptp_report)`
  - Re-issue next read request to BT transport
- Handle edge cases:
  - Zero-length reports (ignore, re-issue read)
  - Unknown report IDs (pass through or ignore)
  - Buffer too small (log warning, re-issue read)

**Verification:** Touch MT2 over Bluetooth. Debug traces show finger data flowing through pipeline. VHF receives PTP reports. Cursor should start moving if Windows PTP client is consuming from the virtual device.

---

### Phase 22: BT Driver — VHF Feature Report Callbacks & Multitouch Activation

**Goal:** VHF virtual device responds to Windows PTP queries. Trackpad multitouch enabled.

**Deliverables:**
- `crates/amt-ptp-bt/src/vhf.rs` — callback implementations:
  - `on_get_feature(context, op_handle, op_context, xfer_packet)`:
    - Read `report_id` from `xfer_packet`
    - Delegate to `amt_ptp_core::handle_get_feature()`
    - Copy result into `xfer_packet.reportBuffer`
    - `VhfAsyncOperationComplete(op_handle, STATUS_SUCCESS)`
  - `on_set_feature(context, op_handle, op_context, xfer_packet)`:
    - Read report_id and data from `xfer_packet`
    - Delegate to `amt_ptp_core::handle_set_feature()`
    - When input mode set to PTP (mode=3):
      - Send BT multitouch enable: feature report 0xF1 = [0xF1, 0x02, 0x01] to real device
      - Start issuing read requests to BT transport
    - `VhfAsyncOperationComplete(op_handle, STATUS_SUCCESS)`
- Multitouch activation sequence:
  - On `EvtDeviceSelfManagedIoInit` after VHF start:
    - Send feature 0xF1 to BT transport to enable multitouch
    - Begin continuous read request chain

**Verification:** Windows PTP client queries device caps → gets max_contacts=5, clickpad. Input mode set to PTP. Multitouch data flows. Full touch/gesture functionality works.

---

### Phase 23: BT Driver — Recovery, INF, Build & Testing

**Goal:** Robust BT driver with error recovery. Complete installable package. End-to-end test.

**Deliverables:**
- Recovery mechanisms in `transport.rs`:
  - Timer-based retry (2-3 second WDF timer) when read requests fail
  - Work item for re-issuing reads after spurious completions
  - Handle BT disconnection gracefully (stop reads, VHF continues to exist)
  - Handle BT reconnection (re-enable multitouch, restart reads)
- `crates/amt-ptp-bt/amt-ptp-bt.inx`:
  - Hardware ID: `HID\{00001124-0000-1000-8000-00805f9b34fb}_VID&0001004c_PID&0265&Col01`
  - Upper/lower filter registration with `vhf` as lower filter
  - Service configuration
  - Col02 null device entry (suppress auxiliary collection)
- Build verification:
  - `cargo wdk build` produces: `amt_ptp_bt.sys`, INF, CAT
  - Test-signed
- Testing checklist:
  - [ ] Driver installs on BT MT2 without errors
  - [ ] Virtual PTP device appears in Device Manager
  - [ ] Single finger move → cursor moves
  - [ ] Two-finger scroll works
  - [ ] Three-finger gestures work
  - [ ] Pinch-to-zoom works
  - [ ] Physical click works
  - [ ] Tap-to-click works
  - [ ] Sleep/resume: trackpad recovers
  - [ ] BT disconnect/reconnect: trackpad recovers
  - [ ] Power cycle trackpad: device re-enumerates and works
  - [ ] No BSOD under stress (rapid connect/disconnect)

**Verification:** All checklist items pass. BT driver is feature-complete for MT2.

---

### Phase 24: Magic Trackpad 3, Settings App & Final Packaging

**Goal:** MT3 support, settings app, unified driver package, documentation.

**Deliverables:**

**MT3 Support:**
- Reverse-engineer MT3 protocol with user's hardware:
  - USB descriptor dump → identify PID
  - USBPcap trace → verify TYPE5 format
  - Compare coordinate ranges, header sizes, multitouch enable sequence
- Add `MAGIC_TRACKPAD_3` config to `amt-ptp-core`
- Update INF files with MT3 hardware IDs (USB + BT)
- Test both transports on MT3

**Settings App (`amt-ptp-settings`):**
- Technology: Rust + `windows-rs` (Win32 GUI) or egui
- Device discovery via HID device enumeration
- Battery status reading (report ID 0x90)
- Configuration via feature report 0x09:
  - Pressure qualification level
  - Single/multi-contact size qualification level
- Real-time device status display

**Unified Package:**
- Combined INF routing MT2 + MT3 USB and BT to correct drivers
- Driver signing with production certificate (if available)
- Installer (optional): `cargo wdk` package output or custom NSIS/WiX installer
- `README.md` with installation instructions, supported devices, troubleshooting

**Defuzz Filter (optional):**
- Implement contact position smoothing in `amt-ptp-core`
- Configurable via settings app
- Simple exponential moving average or Kalman filter on X/Y

**Documentation:**
- Architecture overview in `README.md`
- Per-crate `README.md` with API documentation
- `cargo doc` generates full API docs
- Contributing guide

**Verification:** Both MT2 and MT3 work over both USB and BT. Settings app communicates with drivers. Package installs cleanly on a fresh Windows 11 22H2 machine.

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
