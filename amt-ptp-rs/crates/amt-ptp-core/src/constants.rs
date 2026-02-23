//! Hardware constants for Apple trackpad devices and Windows PTP reports.
//!
//! All values are derived from the original C driver headers:
//! - `AppleDefinition.h` (USB KM / USB UM / SPI KM)
//! - `HidCommon.h`
//! - `Hid.h`

// ── Apple Hardware ──────────────────────────────────────────────────

/// Apple USB vendor ID.
pub const USB_VENDOR_ID_APPLE: u16 = 0x05ac;

/// Apple Bluetooth vendor ID (as seen in BT SDP hardware ID strings).
pub const BT_VENDOR_ID_APPLE: u16 = 0x004c;

// ── Product IDs ─────────────────────────────────────────────────────

/// Magic Trackpad 2 (2015), USB and Bluetooth.
pub const PID_MAGIC_TRACKPAD2: u16 = 0x0265;

// T2 chip trackpads (MacBook Pro/Air 2018-2020)
/// MacBook Pro 13″ 2018 (T2).
pub const PID_T2_7A: u16 = 0x027a;
/// MacBook Pro 13″ 2019 (T2).
pub const PID_T2_7B: u16 = 0x027b;
/// MacBook Pro 15″ 2018 (T2).
pub const PID_T2_7C: u16 = 0x027c;
/// MacBook Pro 15″ 2019 (T2).
pub const PID_T2_7D: u16 = 0x027d;
/// MacBook Pro 2018 variant (T2).
pub const PID_T2_73: u16 = 0x0273;
/// MacBook Pro 2018 variant (T2).
pub const PID_T2_74: u16 = 0x0274;
/// MacBook Air 2018 (T2).
pub const PID_T2_77: u16 = 0x0277;
/// MacBook Pro 16″ (T2).
pub const PID_T2_7E: u16 = 0x027e;
/// MacBook Pro 16″ variant (T2).
pub const PID_T2_7F: u16 = 0x027f;
/// MacBook Air 2020 / MacBook Pro variant (T2).
pub const PID_T2_80: u16 = 0x0280;
/// MacBook Air 2020 variant (T2).
pub const PID_T2_90: u16 = 0x0290;
/// MacBook Air 2020 variant (T2).
pub const PID_T2_91: u16 = 0x0291;
/// MacBook (T2 variant).
pub const PID_T2_340: u16 = 0x0340;

/// Fallback config for unknown T2 devices.
pub const PID_DEFAULT_FALLBACK: u16 = 0xffff;

// ── Trackpad Type Constants ─────────────────────────────────────────

/// Maximum raw fingers reported by Apple firmware.
pub const MAX_FINGERS: usize = 16;

/// Maximum finger orientation value (±16384).
pub const MAX_FINGER_ORIENTATION: i32 = 16384;

/// Mouse report size in bytes.
pub const MOUSE_REPORT_SIZE: usize = 8;

// ── Header Sizes (bytes) ────────────────────────────────────────────

/// TYPE1 header size: 13 × sizeof(u16) = 26.
pub const HEADER_TYPE1: usize = 13 * 2;
/// TYPE2 header size: 15 × sizeof(u16) = 30.
pub const HEADER_TYPE2: usize = 15 * 2;
/// TYPE3 header size: 19 × sizeof(u16) = 38.
pub const HEADER_TYPE3: usize = 19 * 2;
/// TYPE4 header size: 23 × sizeof(u16) = 46.
pub const HEADER_TYPE4: usize = 23 * 2;
/// TYPE5 USB header size: 6 × sizeof(u16) = 12.
pub const HEADER_TYPE5_USB: usize = 6 * 2;
/// TYPE5 Bluetooth header size: 2 × sizeof(u16) = 4.
pub const HEADER_TYPE5_BT: usize = 2 * 2;

// ── Button Offsets (byte offset into report) ────────────────────────

/// TYPE1 button byte offset.
pub const BUTTON_TYPE1: usize = 0;
/// TYPE2 button byte offset.
pub const BUTTON_TYPE2: usize = 15;
/// TYPE3 button byte offset.
pub const BUTTON_TYPE3: usize = 23;
/// TYPE4 button byte offset.
pub const BUTTON_TYPE4: usize = 31;
/// TYPE5 button byte offset.
pub const BUTTON_TYPE5: usize = 1;

// ── Finger Block Size (bytes per finger) ────────────────────────────

/// TYPE1-3 finger block: 14 × sizeof(u16) = 28.
pub const FSIZE_TYPE1: usize = 14 * 2;
/// TYPE2 finger block: 14 × sizeof(u16) = 28.
pub const FSIZE_TYPE2: usize = 14 * 2;
/// TYPE3 finger block: 14 × sizeof(u16) = 28.
pub const FSIZE_TYPE3: usize = 14 * 2;
/// TYPE4 finger block: 15 × sizeof(u16) = 30.
pub const FSIZE_TYPE4: usize = 15 * 2;
/// TYPE5 finger block: 9 bytes (packed).
pub const FSIZE_TYPE5: usize = 9;

// ── Finger Data Delta (offset from header end to first finger) ──────

/// TYPE1-3 delta: 0 bytes.
pub const DELTA_TYPE1: usize = 0;
/// TYPE2 delta: 0 bytes.
pub const DELTA_TYPE2: usize = 0;
/// TYPE3 delta: 0 bytes.
pub const DELTA_TYPE3: usize = 0;
/// TYPE4 delta: 1 × sizeof(u16) = 2 bytes.
pub const DELTA_TYPE4: usize = 2;
/// TYPE5 delta: 0 bytes.
pub const DELTA_TYPE5: usize = 0;

// ── USB Control Message (Wellspring Mode) ───────────────────────────

/// Wellspring mode read request ID.
pub const WELLSPRING_MODE_READ_REQUEST_ID: u8 = 1;
/// Wellspring mode write request ID.
pub const WELLSPRING_MODE_WRITE_REQUEST_ID: u8 = 9;

// ── Signal-to-Noise Ratios ──────────────────────────────────────────

/// Pressure signal-to-noise ratio.
pub const SN_PRESSURE: i32 = 45;
/// Width signal-to-noise ratio.
pub const SN_WIDTH: i32 = 25;
/// Coordinate signal-to-noise ratio.
pub const SN_COORD: i32 = 250;
/// Orientation signal-to-noise ratio.
pub const SN_ORIENT: i32 = 10;

// ── Touch Qualification Thresholds ──────────────────────────────────

/// Single-finger pressure qualification threshold.
pub const PRESSURE_QUALIFICATION_THRESHOLD: u8 = 2;
/// Single-finger size qualification threshold.
pub const SIZE_QUALIFICATION_THRESHOLD: u8 = 9;
/// Multi-finger size lower threshold.
pub const SIZE_MU_LOWER_THRESHOLD: u8 = 5;
/// Multi-finger total pressure qualification threshold.
pub const PRESSURE_MU_QUALIFICATION_THRESHOLD_TOTAL: u8 = 15;
/// Multi-finger total size qualification threshold.
pub const SIZE_MU_QUALIFICATION_THRESHOLD_TOTAL: u8 = 25;

// ── Windows PTP Report IDs ──────────────────────────────────────────

/// Standard mouse report (not used in PTP mode).
pub const REPORTID_MOUSE: u8 = 0x02;
/// Multitouch input report (the main PTP touch data).
pub const REPORTID_MULTITOUCH: u8 = 0x05;
/// Input mode feature report (mouse vs. multitouch).
pub const REPORTID_INPUT_MODE: u8 = 0x04;
/// PTP HQA certification blob feature report.
pub const REPORTID_PTPHQA: u8 = 0x08;
/// Selective reporting (button/surface switch) feature report.
pub const REPORTID_FUNC_SWITCH: u8 = 0x06;
/// Device capabilities feature report.
pub const REPORTID_DEVICE_CAPS: u8 = 0x07;
/// User-mode app configuration feature report.
pub const REPORTID_UMAPP_CONF: u8 = 0x09;

// ── PTP Constants ───────────────────────────────────────────────────

/// Maximum contact points reported in a single PTP input report.
pub const PTP_MAX_CONTACT_POINTS: usize = 5;

/// Clickpad (integrated button in trackpad surface).
pub const PTP_BUTTON_TYPE_CLICK_PAD: u8 = 0;
/// Pressure pad (force-sensing, no physical click).
pub const PTP_BUTTON_TYPE_PRESSURE_PAD: u8 = 1;

/// Mouse collection mode.
pub const PTP_COLLECTION_MOUSE: u8 = 0;
/// Windows Precision Touchpad collection mode.
pub const PTP_COLLECTION_WINDOWS: u8 = 3;

/// Driver/device version reported in HID descriptor.
pub const DEVICE_VERSION: u8 = 0x01;

// ── HID Usage Constants ─────────────────────────────────────────────

/// Digitizer usage: Button Switch.
pub const HID_USAGE_BUTTON_SWITCH: u8 = 0x57;
/// Digitizer usage: Surface Switch.
pub const HID_USAGE_SURFACE_SWITCH: u8 = 0x58;
