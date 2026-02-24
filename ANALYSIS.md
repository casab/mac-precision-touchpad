# Mac Precision Touchpad - Complete Codebase Analysis

## Project Overview

**Original Author:** Bingxing Wang (imbushuo)
**Purpose:** Windows Precision Touchpad (PTP) protocol implementation for Apple MacBook trackpads and Magic Trackpad 2/3
**Licensing:** GPLv2 (USB driver), MIT (SPI driver)

---

## Architecture Overview

The project is a multi-transport Windows driver package that translates Apple's proprietary trackpad protocols into Microsoft's Windows Precision Touchpad (PTP) HID standard. It consists of 4 driver modules and 1 UWP settings app.

```
┌──────────────────────────────────────────────────────┐
│              Windows PTP HID Stack (OS)               │
├───────────┬───────────┬──────────────┬───────────────┤
│ HID Filter│ SPI KM    │ USB KM       │ USB UM        │
│ (BT MT2)  │ (MacBook  │ (T2 Macs)    │ (Legacy USB   │
│           │  SPI)     │              │  + MT2 USB)   │
├───────────┴───────────┴──────────────┴───────────────┤
│         Apple Hardware (BCM5974 / SPI / BT)          │
└──────────────────────────────────────────────────────┘
```

---

## Module Details

### Module 1: AmtPtpDeviceUsbUm (USB User-Mode Driver)

**Framework:** UMDF v2.15 | **Transport:** USB | **Target:** Legacy MacBook trackpads + Magic Trackpad 2 USB

The most complete and mature driver. Handles 15 Wellspring device generations.

#### Key Source Files

| File | Lines | Purpose |
|------|-------|---------|
| `src/AmtPtpDeviceUsbUm/Driver.c` | Entry point, device add |
| `src/AmtPtpDeviceUsbUm/Device.c` | ~800 lines | Device init, hardware prep, Wellspring mode, D0 entry/exit, USB interface selection |
| `src/AmtPtpDeviceUsbUm/InputInterrupt.c` | ~598 lines | Core touch input - TYPE1-4 and TYPE5 (MT2) handlers |
| `src/AmtPtpDeviceUsbUm/Hid.c` | HID descriptor serving, feature reports |
| `src/AmtPtpDeviceUsbUm/Queue.c` | IOCTL dispatch, manual input queue |

#### Device Configuration Table

The `BCM5974_CONFIG` table in `include/AppleDefinition.h` maps 15 device families:

```c
struct BCM5974_CONFIG {
    int ansi, iso, jis;         // Product IDs for keyboard layout variants
    int caps;                   // Capability bitmask
    int bt_ep, bt_datalen;      // Button endpoint
    int tp_ep;                  // Trackpad endpoint
    enum TRACKPAD_TYPE tp_type; // TYPE1-TYPE5
    int tp_header;              // Header size in bytes
    int tp_datalen;             // Data length
    int tp_button;              // Button offset
    int tp_fsize;               // Finger struct size
    int tp_delta;               // Header-to-finger offset
    int um_size;                // USB control message length
    int um_req_val;             // USB control message value
    int um_req_idx;             // USB control message index
    int um_switch_idx;          // Mode switch index
    int um_switch_on;           // Mode switch on value
    int um_switch_off;          // Mode switch off value
    struct BCM5974_PARAM p;     // Pressure limits
    struct BCM5974_PARAM w;     // Width limits
    struct BCM5974_PARAM x;     // Horizontal limits (min/max)
    struct BCM5974_PARAM y;     // Vertical limits (min/max)
    struct BCM5974_PARAM o;     // Orientation limits
};
```

Device table entries (USB UM):
- Wellspring 1 (MacbookAir): 0x0223/0224/0225, TYPE1
- Wellspring 2 (MacbookProPenryn): 0x0230/0231/0232, TYPE1
- Wellspring 3 (Macbook5,1 unibody): 0x0236/0237/0238, TYPE2
- Wellspring 4 (MacbookAir3,2): 0x023f/0240/0241, TYPE2
- Wellspring 4A (MacbookAir3,1): 0x0242/0243/0244, TYPE2
- Wellspring 5 (Macbook8): 0x0245/0246/0247, TYPE2
- Wellspring 6A (MacbookAir4,1): 0x0249/024a/024b, TYPE2
- Wellspring 6 (MacbookAir4,2): 0x024c/024d/024e, TYPE2
- Wellspring 5A (Macbook8,2): 0x0252/0253/0254, TYPE2
- Wellspring 7 (MacbookPro10,1): 0x0262/0263/0264, TYPE2
- Wellspring 7A (MacbookPro10,2): 0x0259/025a/025b, TYPE2
- Wellspring 8 (MacbookAir6,2): 0x0290/0291/0292, TYPE3
- Wellspring 9 (MacbookPro12,1): 0x0272/0273/0274, TYPE4
- Magic Trackpad 2: 0x0265, TYPE5
- Apple T2: 0x027d, TYPE4

#### Wellspring Mode Switching

Apple trackpads default to "basic mouse" mode. To get raw multitouch, the driver sends USB control transfers:
1. Read current mode: `BmRequestDeviceToHost`, request ID 1, class request
2. Modify mode byte at `um_switch_idx` position
3. Write back: `BmRequestHostToDevice`, request ID 9, class request
4. TYPE3 devices skip this (always multitouch)

#### Trackpad Finger Data Structures

**Types 1-4** (`TRACKPAD_FINGER`, 28-30 bytes, le16-aligned):
```c
struct TRACKPAD_FINGER {
    USHORT origin;       // zero when switching track finger
    USHORT abs_x;        // absolute x coordinate
    USHORT abs_y;        // absolute y coordinate
    USHORT rel_x;        // relative x coordinate
    USHORT rel_y;        // relative y coordinate
    USHORT tool_major;   // tool area, major axis
    USHORT tool_minor;   // tool area, minor axis
    USHORT orientation;  // 16384 when point, else 15 bit angle
    USHORT touch_major;  // touch area, major axis
    USHORT touch_minor;  // touch area, minor axis
    USHORT unused[2];    // zeros
    USHORT pressure;     // pressure on forcetouch touchpad
    USHORT multi;        // one finger: varies, more: constant
};
```

**Type 5 / Magic Trackpad 2** (`TRACKPAD_FINGER_TYPE5`, 9 bytes):
```c
struct TRACKPAD_FINGER_TYPE5 {
    UCHAR AbsoluteX;     // absolute x coordinate (low bits)
    UCHAR AbsoluteXY;    // absolute x,y coordinate (mixed)
    UCHAR AbsoluteY[2];  // absolute y coordinate
    UCHAR TouchMajor;    // touch area, major axis
    UCHAR TouchMinor;    // touch area, minor axis
    UCHAR Size;           // tool area, size
    UCHAR Pressure;       // pressure
    union {
        struct {
            UCHAR Id : 4;          // contact identifier
            UCHAR Orientation : 4;  // orientation
        } ContactIdentifier;
        UCHAR RawOrientationAndOrigin;
    };
};
```

#### Touch Input Processing

**Types 2/3/4 coordinate translation:**
```
x = (raw_x - x.min) clamped to >= 0
y = (y.max - raw_y) clamped to >= 0  (Y-axis inversion)
TipSwitch = (touch_major << 1) >= 200
Confidence = (touch_minor << 1) > 0
```

**Type 5 / Magic Trackpad 2 coordinate extraction:**
```
tmp_x = (*(USHORT*)finger) & 0x1fff          // 13-bit X
tmp_y = (INT)(*(UINT*)finger)
x = (SHORT)(tmp_x << 3) >> 3                 // sign-extend 13-bit to 16-bit
y = -(INT)(tmp_y << 6) >> 19                 // extract and sign-extend Y
x = (x - x.min) clamped to >= 0
y = (y - y.min) clamped to >= 0
ContactID = finger->ContactIdentifier.Id     // 4-bit field
TipSwitch = (TouchMajor << 1) > 0
Confidence = (TouchMinor << 1) < 345         // raised threshold for Apple
```

#### Scan Time Calculation
Performance counter delta / 100 = time in 100us units, capped at 0xFF.

---

### Module 2: AmtPtpDeviceUsbKm (USB Kernel-Mode Driver)

**Framework:** KMDF | **Transport:** USB | **Target:** Apple T2 Macs (2018-2020)

Structurally similar to USB UM but kernel-mode, with these differences:
- Operates as a filter driver (WdfFdoInitSetFilter)
- Simpler config: only T2 product IDs (0x027A-0x027D) + fallback
- Single product ID per config entry (not ansi/iso/jis triplet)
- Uses KeQueryPerformanceCounter instead of QueryPerformanceCounter
- Combined TipSwitch: `(touch_major << 1) >= 200 || (touch_minor << 1) >= 150`

#### T2 Configuration Table
```c
{ USB_DEVICE_ID_APPLE_T2_7A, ..., DATAFORMAT(TYPE4), x: {-6243, 6749}, y: {-170, 7685} }
{ USB_DEVICE_ID_APPLE_T2_7B, ..., DATAFORMAT(TYPE4), x: {-6243, 6749}, y: {-170, 7685} }
{ USB_DEVICE_ID_APPLE_T2_7C, ..., DATAFORMAT(TYPE4), x: {-10000, 10000}, y: {-2000, 10000} }
{ USB_DEVICE_ID_APPLE_T2_7D, ..., DATAFORMAT(TYPE4), x: {-10000, 10000}, y: {-2000, 10000} }
{ USB_DEVICE_ID_DEFAULT_FALLBACK, ..., DATAFORMAT(TYPE4), x: {-10000, 10000}, y: {-2000, 10000} }
```

---

### Module 3: AmtPtpDeviceSpiKm (SPI Kernel-Mode Driver)

**Framework:** KMDF v1.23 | **Transport:** SPI | **Target:** MacBook 2015-2017 (non-T2)

#### SPI-specific packet format
```c
typedef struct _SPI_TRACKPAD_FINGER {
    SHORT OriginalX, OriginalY, X, Y;
    SHORT HorizontalAccel, VerticalAccel;
    SHORT ToolMajor, ToolMinor, Orientation;
    SHORT TouchMajor, TouchMinor;
    SHORT Rsvd1, Rsvd2, Pressure, Rsvd3;
} SPI_TRACKPAD_FINGER;

typedef struct _SPI_TRACKPAD_PACKET {
    UINT8 PacketType, ClickOccurred;
    UINT8 Reserved0[5], IsFinger, Reserved1[16];
    UINT8 FingerDataLength, Reserved2[5], NumOfFingers;
    UINT8 ClickOccurred2, State1-3, Padding, Reserved3[10];
    SPI_TRACKPAD_FINGER Fingers[10];
} SPI_TRACKPAD_PACKET;
```

#### SPI Mode Switch
Uses `SPI_SET_FEATURE` with `IOCTL_HID_SET_FEATURE`:
- BusLocation = 2
- Status = 1 (on) or 0 (off)
- Report ID = 2 (HID_REPORTID_MOUSE)

#### SPI Config Table
```
MacBookPro11,1/12,1: PID 0x0272/0x0273, X: [-4750, 5280], Y: [-150, 6730]
MacBook9:            PID 0x0275, X: [-5087, 5579], Y: [-128, 6089]
MacBookPro14,1/14,2: PID 0x0276/0x0277, X: [-6243, 6749], Y: [-170, 7685]
MacBookPro14,3:      PID 0x0278, X: [-7456, 7976], Y: [-163, 9283]
MacBook10:           PID 0x0279, X: [-5087, 5579], Y: [-128, 6089]
MacBookAir7,2:       PID 0x0290/0x0291, X: [-5087, 5579], Y: [-128, 6089]
```

#### Power State Machine
```
D3 -> D0ActiveAndUnconfigured -> D0ActiveAndConfigured
                                      |
                            (SPI state switch, retry timer after 5s on failure)
```

---

### Module 4: AmtPtpHidFilter (HID Filter Driver)

**Framework:** KMDF v1.15 | **Transport:** Bluetooth HID | **Target:** Magic Trackpad 2 Bluetooth

Most architecturally complex module. Implements its own HID miniport transport while reusing Bluetooth HID transport.

#### Stack Architecture
```
OS HID Stack
    ↕
mshidkmdf (Microsoft HID KMDF miniport host)
    ↕
AmtPtpHidFilter (Lower filter)
    ↕
Bluetooth HID Transport
    ↕
Magic Trackpad 2
```

#### HID Stack Detour (Detour.c)

The driver patches the Windows HID stack by:
1. Gets WDM device object for self
2. Finds lower device (HID transport) via `IoGetLowerDeviceObject`
3. Navigates to driver extension -> IO client extension
4. Verifies "HIDCLASS" identifier
5. Finds `HIDCLASS_DRIVER_EXTENSION` with original MajorFunction table
6. Replaces `IRP_MJ_INTERNAL_DEVICE_CONTROL` with the original transport handler
7. Uses the detoured IO target for direct HID transport communication

Critical structures for the hack:
```c
typedef struct _HIDCLASS_DRIVER_EXTENSION {
    PDRIVER_OBJECT      MinidriverObject;
    UNICODE_STRING      RegistryPath;
    ULONG               DeviceExtensionSize;
    PDRIVER_DISPATCH    MajorFunction[IRP_MJ_MAXIMUM_FUNCTION + 1];
    PDRIVER_ADD_DEVICE  AddDevice;
    PDRIVER_UNLOAD      DriverUnload;
    LONG                ReferenceCount;
    LIST_ENTRY          ListEntry;
    BOOLEAN             DevicesArePolled;
} HIDCLASS_DRIVER_EXTENSION;

typedef struct _IO_CLIENT_EXTENSION {
    struct _IO_CLIENT_EXTENSION* NextExtension;
    PVOID ClientIdentificationAddress;  // Should be "HIDCLASS"
} IO_CLIENT_EXTENSION;
```

#### Magic Trackpad 2 Configuration

USB VID: 0x05AC (Apple USB), BT VID: 0x004C (Apple BT)
Product ID: 0x0265

USB configuration:
- InputFingerSize = FSIZE_TYPE5 (9 bytes)
- InputHeaderSize = HOFFSET_TYPE_USB_5 (6 * sizeof(USHORT) = 12 bytes)
- InputFingerDelta = FDELTA_TYPE5 (0)
- InputButtonDelta = BOFFSET_TYPE5 (1)
- X: min=-3678, max=3934, snratio=250
- Y: min=-2479, max=2586, snratio=250
- Multitouch enable: reportId=0x02, data=[0x02, 0x01, 0x00, 0x00]

Bluetooth configuration:
- Same finger/coordinate params
- InputHeaderSize = HOFFSET_TYPE_BTH_5 (2 * sizeof(USHORT) = 4 bytes)
- Multitouch enable: reportId=0xF1, data=[0xF1, 0x02, 0x01]

#### HID Filter Input Processing
Same TYPE5 bit manipulation as USB UM driver:
```
tmp_x = (*(USHORT*)f_type5) & 0x1fff
tmp_y = (INT)(*(unsigned int*)f_type5)
x = (SHORT)(tmp_x << 3) >> 3
y = -(INT)(tmp_y << 6) >> 19
```

#### Recovery Mechanisms
- Timer-based retry (2-3 seconds) on transport failures
- Work item for re-issuing read requests after spurious completions
- Lookaside list for HID read buffer allocation

---

### Module 5: AmtPtpDevice.Settings (UWP App)

C#/XAML UWP application for Magic Trackpad 2 configuration.

#### Functionality
- Device discovery via HidDevice.GetDeviceSelector watchers
- Battery status reading (report ID 0x90, usage page 0xFF00/0x0014)
- Configuration via feature report 0x09 (REPORTID_UMAPP_CONF):
  - PressureQualificationLevel
  - SingleContactSizeQualificationLevel
  - MultipleContactSizeQualificationLevel
- Auto-reconnect on device loss/reappearance

---

## HID Protocol Implementation

### PTP Report Structure (all modules)

```c
typedef struct _PTP_CONTACT {
    UCHAR   Confidence : 1;
    UCHAR   TipSwitch : 1;
    UCHAR   Padding : 6;
    ULONG   ContactID;        // Note: SPI uses 3-bit bitfield instead
    USHORT  X;
    USHORT  Y;
} PTP_CONTACT;

typedef struct _PTP_REPORT {
    UCHAR       ReportID;      // 0x05
    PTP_CONTACT Contacts[5];
    USHORT      ScanTime;      // in 100us units
    UCHAR       ContactCount;
    UCHAR       IsButtonClicked;
} PTP_REPORT;
```

### Feature Reports

| Report ID | Name | Purpose | Size |
|-----------|------|---------|------|
| 0x04 | REPORTID_REPORTMODE | Input mode: Mouse=0, PTP=3 | 2 bytes |
| 0x05 | REPORTID_MULTITOUCH | Touch input report | ~30 bytes |
| 0x06 | REPORTID_FUNCSWITCH | Button/Surface enable bits | 2 bytes |
| 0x07 | REPORTID_DEVICE_CAPS | Max contacts=5, type=clickpad | 3 bytes |
| 0x08 | REPORTID_PTPHQA | HQA certification blob | 257 bytes |
| 0x09 | REPORTID_UMAPP_CONF | User-mode app config | 4 bytes |

### HID Report Descriptors

Each driver module provides device-specific HID report descriptors as static byte arrays built from C macros. The descriptors define:
- Digitizer usage page (0x0D)
- Touch Pad usage (0x05)
- 5 finger collections with Confidence, TipSwitch, ContactID, X, Y
- Scan Time (100us units)
- Contact Count
- Button (1 bit + 7 padding)
- Device Caps feature (max contacts, button type)
- HQA certification feature (256 bytes)
- Configuration TLC (input mode, function switch)

Physical/logical maximums are device-specific:
- Magic Trackpad 2: X logical max 7612, X physical max 1600 (160.0mm), Y logical max 5065, Y physical max 1149 (114.9mm)
- SPI Series 3 13": X logical max 12992, X physical max 1352, Y logical max 7855, Y physical max 836
- SPI Series 3 15": X logical max 15432, X physical max 1584, Y logical max 9446, Y physical max 992

### HQA Certification Blob

A hardcoded 256-byte blob used across all driver modules:
```
fc 28 fe 84 40 cb 9a 87 0d be 57 3c b6 70 09 88
07 97 2d 2b e3 38 34 b6 6c ed b0 f7 e5 9c f6 c2
... (256 bytes total)
```

---

## INF Package (AmtPtpDeviceUniversalPkg)

### Hardware ID Routing

| Category | Hardware IDs | Driver |
|----------|-------------|--------|
| Apple T2 USB | USB\Vid_05ac&Pid_027A-0291&MI_02 (13 entries) | AmtPtpDeviceUsbKm.sys |
| Legacy USB MacBook | USB\Vid_05ac&Pid_0236-0274&MI_01/02 (25 entries) | AmtPtpDeviceUsbUm.dll |
| SPI MacBook | SPI\VID_05ac&PID_0272-0291&MI_02 (10 entries) | AmtPtpDeviceSpiKm.sys |
| Magic Trackpad 2 USB | USB\Vid_05ac&Pid_0265&MI_01 | AmtPtpDeviceUsbUm.dll |
| Magic Trackpad 2 BT | HID\{BT UUID}_VID&0001004c_PID&0265&Col01 | AmtPtpHidFilter.sys |
| BT Col02 (null) | HID\{BT UUID}_VID&0001004c_PID&0265&Col02 | Null device |

---

## Known Issues & Incomplete Features

1. **Defuzz not implemented** - `PTP_CONTACT_RAW` and linked-list structures exist in headers but are unused. Would smooth jittery touch data.

2. **Input sensitivity configuration incomplete** - Listed in roadmap as TODO.

3. **Duplicate confidence check bug** - In `InputInterrupt.c:523` and `Input.c:217` (HID filter), the confidence check duplicates the TouchMinor comparison instead of checking both TouchMinor AND TouchMajor.

4. **Scan time cap too low** - All drivers cap at 0xFF (25.5ms) but the field is 2 bytes (USHORT), could hold up to 0xFFFF.

5. **PTP_CONTACT struct inconsistency** - USB KM uses `ULONG ContactID` (4 bytes), SPI uses 3-bit bitfield, HID filter uses `ULONG ContactID`. The HID descriptor for some modules only defines 3 bits for ContactID while others use 32 bits.

6. **Emergency reset only in USB UM** - USB UM has AmtPtpEmergResetDevice (toggle Wellspring off/on), other modules don't.

7. **SPI touchscreen mode** - Partially implemented but not fully wired up for user configuration.

---

## Relevant Magic Trackpad 2 & 3 Specifics

### Identification
- Vendor ID (USB): 0x05AC
- Vendor ID (Bluetooth): 0x004C
- Product ID: 0x0265 (MT2, likely same for MT3)

### USB Interface
- MI_01 (Interface 1) for USB UM driver
- TYPE5 trackpad format
- Wellspring mode switch: um_size=2, um_req_val=0x302, um_req_idx=1, um_switch_idx=1, um_switch_on=1, um_switch_off=0

### Bluetooth Interface
- Bluetooth UUID: {00001124-0000-1000-8000-00805f9b34fb}
- HID collections: Col01 (touchpad data), Col02 (auxiliary, null device)
- USB HID revision: 0x0855
- Multitouch enable via feature report 0xF1: [0xF1, 0x02, 0x01]

### Coordinate System
- X range: -3678 to 3934 (total 7612 units)
- Y range: -2479 to 2586 (total 5065 units)
- Physical dimensions: 160.0mm x 114.9mm
- Signal-to-noise ratio: 250 for both axes

### Touch Data (TYPE5 format)
- 9 bytes per finger
- Max 5 fingers reported (PTP_MAX_CONTACT_POINTS)
- Header: 12 bytes (USB), 4 bytes (Bluetooth)
- Button offset: byte 1
- Finger delta: 0

### Multitouch Activation
- USB: Set feature report 0x02 with [0x02, 0x01, 0x00, 0x00]
- Bluetooth: Set feature report 0xF1 with [0xF1, 0x02, 0x01]
