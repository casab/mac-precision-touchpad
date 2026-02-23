//! Windows Precision Touchpad (PTP) report types.
//!
//! These are the HID report structures that Windows expects from a
//! Precision Touchpad device. All structs are `#[repr(C, packed)]` to
//! match the exact byte layout defined by the HID report descriptor.
//!
//! Ported from `Hid.h` in the C driver.

use crate::constants::*;

/// A single finger contact in a PTP input report.
///
/// Layout (9 bytes, packed):
/// - Byte 0: Confidence (bit 0) | TipSwitch (bit 1) | padding (bits 2-7)
/// - Bytes 1-4: Contact ID (u32, LE)
/// - Bytes 5-6: X (u16, LE)
/// - Bytes 7-8: Y (u16, LE)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct PtpContact {
    /// Bit 0: Confidence, Bit 1: TipSwitch, Bits 2-7: padding.
    pub flags: u8,
    /// Contact identifier (stable across frames for the same finger).
    pub contact_id: u32,
    /// X coordinate in PTP logical units.
    pub x: u16,
    /// Y coordinate in PTP logical units.
    pub y: u16,
}

impl PtpContact {
    /// Create a new contact with the given parameters.
    #[must_use]
    pub const fn new(contact_id: u32, x: u16, y: u16, confidence: bool, tip_switch: bool) -> Self {
        let flags = (confidence as u8) | ((tip_switch as u8) << 1);
        Self { flags, contact_id, x, y }
    }

    /// Create an empty (zeroed) contact.
    #[must_use]
    pub const fn empty() -> Self {
        Self { flags: 0, contact_id: 0, x: 0, y: 0 }
    }

    /// Whether this contact has the Confidence bit set.
    #[must_use]
    pub const fn confidence(&self) -> bool {
        self.flags & 0x01 != 0
    }

    /// Whether this contact has the TipSwitch bit set.
    #[must_use]
    pub const fn tip_switch(&self) -> bool {
        self.flags & 0x02 != 0
    }
}

/// PTP multitouch input report (Report ID 0x05).
///
/// This is the main touch data report sent to Windows for each frame.
///
/// Layout (50 bytes total):
/// - Byte 0: Report ID (0x05)
/// - Bytes 1-45: 5 × PtpContact (9 bytes each)
/// - Bytes 46-47: Scan time (u16 LE, in 100µs units)
/// - Byte 48: Contact count (actual number of fingers)
/// - Byte 49: Button clicked (0 or 1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct PtpReport {
    /// Report ID: always [`REPORTID_MULTITOUCH`] (0x05).
    pub report_id: u8,
    /// Contact data for up to 5 fingers.
    pub contacts: [PtpContact; PTP_MAX_CONTACT_POINTS],
    /// Scan time in 100µs units since the last report.
    pub scan_time: u16,
    /// Number of valid contacts in this report.
    pub contact_count: u8,
    /// Whether the trackpad button is clicked (0 or 1).
    pub is_button_clicked: u8,
}

impl PtpReport {
    /// Create a new empty PTP report with the correct report ID.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            report_id: REPORTID_MULTITOUCH,
            contacts: [PtpContact::empty(); PTP_MAX_CONTACT_POINTS],
            scan_time: 0,
            contact_count: 0,
            is_button_clicked: 0,
        }
    }

    /// Get this report as a raw byte slice for submission to HID.
    ///
    /// # Safety
    ///
    /// The returned slice borrows `self`. The struct is `repr(C, packed)`,
    /// so this is a valid reinterpretation.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        let ptr = (self as *const Self).cast::<u8>();
        let len = core::mem::size_of::<Self>();
        // SAFETY: PtpReport is repr(C, packed) with no padding, all fields are
        // plain integer types. The slice borrows self with the same lifetime.
        unsafe { core::slice::from_raw_parts(ptr, len) }
    }
}

// ── Feature Reports ─────────────────────────────────────────────────

/// Device capabilities feature report (Report ID 0x07).
///
/// Sent in response to `GET_REPORT` for the device caps report ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct PtpDeviceCapsReport {
    /// Report ID: [`REPORTID_DEVICE_CAPS`] (0x07).
    pub report_id: u8,
    /// Maximum number of simultaneous contact points (typically 5).
    pub max_contact_points: u8,
    /// Button type: 0 = clickpad, 1 = pressure pad.
    pub button_type: u8,
}

impl PtpDeviceCapsReport {
    /// Default capabilities: 5 contacts, clickpad.
    #[must_use]
    pub const fn default_clickpad() -> Self {
        Self {
            report_id: REPORTID_DEVICE_CAPS,
            max_contact_points: PTP_MAX_CONTACT_POINTS as u8,
            button_type: PTP_BUTTON_TYPE_CLICK_PAD,
        }
    }
}

/// Input mode feature report (Report ID 0x04).
///
/// Windows writes this to switch between mouse mode and multitouch mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct PtpInputModeReport {
    /// Report ID: [`REPORTID_INPUT_MODE`] (0x04).
    pub report_id: u8,
    /// Mode: 0 = mouse, 3 = Windows PTP collection.
    pub mode: u8,
}

/// Selective reporting feature report (Report ID 0x06).
///
/// Controls which sub-reports are active (button and/or surface).
/// Matches the HID descriptor: 2 switch bits + 6 padding bits = 1 byte of data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct PtpSelectiveReportingReport {
    /// Report ID: [`REPORTID_FUNC_SWITCH`] (0x06).
    pub report_id: u8,
    /// Bit 0: button report enabled, Bit 1: surface report enabled, Bits 2-7: padding.
    pub switches: u8,
}

impl PtpSelectiveReportingReport {
    /// Whether button reporting is enabled.
    #[must_use]
    pub const fn button_report_on(&self) -> bool {
        self.switches & 0x01 != 0
    }

    /// Whether surface (touch) reporting is enabled.
    #[must_use]
    pub const fn surface_report_on(&self) -> bool {
        self.switches & 0x02 != 0
    }
}

/// HQA certification feature report (Report ID 0x08).
///
/// Contains the 256-byte certification blob required by Windows PTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct PtpHqaCertificationReport {
    /// Report ID: [`REPORTID_PTPHQA`] (0x08).
    pub report_id: u8,
    /// 256-byte HQA certification blob.
    pub blob: [u8; 256],
}

/// User-mode app configuration feature report (Report ID 0x09).
///
/// Allows a companion user-mode app to tune pressure/size thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct PtpUserModeAppConfReport {
    /// Report ID: [`REPORTID_UMAPP_CONF`] (0x09).
    pub report_id: u8,
    /// Pressure qualification level (0-255).
    pub pressure_qualification_level: u8,
    /// Single-contact size qualification level (0-255).
    pub single_contact_size_level: u8,
    /// Multi-contact size qualification level (0-255).
    pub multi_contact_size_level: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    #[test]
    fn ptp_contact_size() {
        // 1 (flags) + 4 (contact_id) + 2 (x) + 2 (y) = 9
        assert_eq!(mem::size_of::<PtpContact>(), 9);
    }

    #[test]
    fn ptp_report_size() {
        // 1 (report_id) + 5 × 9 (contacts) + 2 (scan_time) + 1 (count) + 1 (button) = 50
        assert_eq!(mem::size_of::<PtpReport>(), 50);
    }

    #[test]
    fn ptp_report_as_bytes_length() {
        let report = PtpReport::new();
        assert_eq!(report.as_bytes().len(), 50);
        assert_eq!(report.as_bytes()[0], REPORTID_MULTITOUCH);
    }

    #[test]
    fn ptp_contact_flags() {
        let c = PtpContact::new(42, 100, 200, true, true);
        assert!(c.confidence());
        assert!(c.tip_switch());
        // Copy packed fields to locals before comparing
        let id = { c.contact_id };
        let x = { c.x };
        let y = { c.y };
        assert_eq!(id, 42);
        assert_eq!(x, 100);
        assert_eq!(y, 200);
    }

    #[test]
    fn ptp_contact_no_flags() {
        let c = PtpContact::new(0, 0, 0, false, false);
        assert!(!c.confidence());
        assert!(!c.tip_switch());
    }

    #[test]
    fn device_caps_report_size() {
        assert_eq!(mem::size_of::<PtpDeviceCapsReport>(), 3);
    }

    #[test]
    fn hqa_report_size() {
        // 1 + 256 = 257
        assert_eq!(mem::size_of::<PtpHqaCertificationReport>(), 257);
    }

    #[test]
    fn selective_reporting_report_size() {
        // 1 (report_id) + 1 (switches) = 2
        assert_eq!(mem::size_of::<PtpSelectiveReportingReport>(), 2);
    }

    #[test]
    fn selective_reporting_flags() {
        let r = PtpSelectiveReportingReport {
            report_id: REPORTID_FUNC_SWITCH,
            switches: 0x03, // both button and surface
        };
        assert!(r.button_report_on());
        assert!(r.surface_report_on());
    }

    #[test]
    fn input_mode_report_size() {
        assert_eq!(mem::size_of::<PtpInputModeReport>(), 2);
    }

    #[test]
    fn umapp_conf_report_size() {
        assert_eq!(mem::size_of::<PtpUserModeAppConfReport>(), 4);
    }
}
