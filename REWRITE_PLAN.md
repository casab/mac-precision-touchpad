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
| Min Windows Version | Windows 10 1809 (build 17763) | User requirement |
| Min WDF Version | KMDF 1.27 | Matches Windows 10 1809 |

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

## 9. Implementation Phases

### Phase 1: Foundation (Core + USB)
1. Set up Cargo workspace and build system with `cargo-wdk`
2. Configure `wdk-sys` for KMDF 1.27 targeting
3. Implement `amt-ptp-core`:
   - Device config tables (MT2 first)
   - TYPE5 finger parsing with unit tests
   - PTP report generation with unit tests
   - HID report descriptor builder
   - Feature report structures
4. Implement `amt-ptp-usb`:
   - KMDF driver skeleton (DriverEntry, DeviceAdd)
   - USB device initialization + interface selection
   - Wellspring mode switching
   - Interrupt pipe continuous reader
   - HID minidriver IOCTL handling
   - Touch input processing (calls into core)
5. Write INF file for USB driver
6. Test on MT2 over USB (if hardware available)

### Phase 2: Bluetooth + VHF
1. Create `vhf-sys` crate with FFI bindings
2. Implement `amt-ptp-bt`:
   - KMDF filter driver skeleton
   - VHF virtual device lifecycle
   - BT transport read/write via I/O target
   - VHF callback handlers (feature reports)
   - Multitouch activation over BT
   - Recovery mechanisms (timer-based retry)
3. Write INF file for BT driver
4. Test on MT2 over Bluetooth

### Phase 3: Magic Trackpad 3
1. Reverse-engineer MT3 protocol (with user's MT3 hardware):
   - Identify product ID
   - Verify TYPE5 format compatibility
   - Determine coordinate ranges
   - Check for any protocol differences
2. Add MT3 config to `amt-ptp-core`
3. Update INF files with MT3 hardware IDs
4. Test both transports on MT3

### Phase 4: Polish
1. Settings app
2. Defuzz filter implementation
3. Input sensitivity configuration
4. Driver signing and packaging
5. Documentation

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
| VHF doesn't work as BT filter pattern | Architecture change | Prototype VHF integration early (Phase 2 task 1); fallback to cleaned-up detour in Rust |
| KMDF 1.27 targeting issues with WDK 22H2 | Can't target Win10 1809 | Modify `wdk-sys` build.rs per issue #149; test on 1809 early |
| MT3 protocol differs significantly from MT2 | Extra reverse-engineering | Start with MT2 only; MT3 is Phase 3 |
| Nightly Rust toolchain instability | Build breaks | Pin to known-good nightly version in `rust-toolchain.toml` |
| Driver signing for distribution | Can't distribute to users | Test-sign during development; EV cert for production |

---

## 12. Open Questions (to resolve during implementation)

1. **MT3 Product ID**: Need to identify via USB descriptor dump from user's MT3
2. **MT3 protocol differences**: May need USBPcap/Wireshark captures
3. **VHF + BT filter interaction**: Need to prototype whether VHF can be used from a filter driver position (vs function driver)
4. **HID descriptor ContactID width**: Should we use 4-bit (matching TYPE5 wire format) or 32-bit (matching original C driver)? PTP spec allows either.
5. **Settings app technology**: Rust GUI vs C# WinUI — defer to Phase 2
