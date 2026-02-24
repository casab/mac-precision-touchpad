//! Raw Apple finger data types and coordinate extraction.
//!
//! Apple trackpads send touch data in device-specific packed formats.
//! This module handles TYPE5 (Magic Trackpad 2) and TYPE2-4 (Wellspring/T2)
//! finger data extraction.
//!
//! ## TYPE5 Memory Layout (9 bytes, packed)
//!
//! ```text
//! Byte 0:    AbsoluteX[7:0]       ← X bits 0-7
//! Byte 1:    AbsoluteXY           ← X bits 8-12 (low 5) | Y bits 0-2 (high 3)
//! Byte 2-3:  AbsoluteY[15:0]      ← Y bits 3-14 via combined extraction
//! Byte 4:    TouchMajor           ← touch area major axis
//! Byte 5:    TouchMinor           ← touch area minor axis
//! Byte 6:    Size                 ← tool area size
//! Byte 7:    Pressure             ← pressure on force-touch surface
//! Byte 8:    Id[3:0] | Orientation[7:4]  ← 4-bit contact ID + 4-bit orientation
//! ```
//!
//! Coordinates are extracted as 13-bit signed values packed across
//! bytes 0-3: X in bits [0:12], Y in bits [13:25] (sign-extended, Y negated).

use crate::device::DeviceConfig;
use crate::error::Error;

/// Parsed finger data from an Apple trackpad report.
///
/// This is the output of parsing — coordinates are still in Apple raw space.
/// Use [`transform_to_ptp`] to convert to Windows PTP logical coordinates.
///
/// [`transform_to_ptp`]: Self::transform_to_ptp
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finger {
    /// Raw X coordinate (Apple space, signed).
    pub raw_x: i32,
    /// Raw Y coordinate (Apple space, signed).
    pub raw_y: i32,
    /// Touch area major axis.
    pub touch_major: u8,
    /// Touch area minor axis.
    pub touch_minor: u8,
    /// Tool area size.
    pub size: u8,
    /// Pressure value (0-255).
    pub pressure: u8,
    /// Contact identifier (0-15 for TYPE5).
    pub contact_id: u8,
    /// Orientation (0-15 for TYPE5, scaled for TYPE2-4).
    pub orientation: u8,
}

impl Finger {
    /// Transform raw Apple coordinates to PTP logical coordinates.
    ///
    /// PTP X = raw_x - config.x.min, clamped to ≥ 0.
    /// PTP Y = raw_y - config.y.min, clamped to ≥ 0.
    ///
    /// For TYPE5, the Y extraction already negates, so no additional
    /// Y-axis inversion is needed.
    #[must_use]
    pub fn transform_to_ptp(&self, config: &DeviceConfig) -> (u16, u16) {
        let x = (self.raw_x - config.x.min).max(0) as u16;
        let y = (self.raw_y - config.y.min).max(0) as u16;
        (x, y)
    }

    /// Whether this finger is touching the surface (TipSwitch).
    ///
    /// TYPE5: `(touch_major << 1) > 0` — any nonzero touch area.
    #[must_use]
    pub const fn is_tip_switch(&self) -> bool {
        (self.touch_major as u16) << 1 > 0
    }

    /// Whether this contact should be reported with confidence.
    ///
    /// Windows PTP spec rejects contacts larger than 25mm. The original driver
    /// uses a threshold of 345 (in shifted units) for Magic Trackpad 2.
    #[must_use]
    pub const fn is_confident(&self) -> bool {
        ((self.touch_minor as u16) << 1) < 345
    }
}

/// Extract TYPE5 (Magic Trackpad 2) finger data from a raw 9-byte block.
///
/// # Arguments
/// * `data` — exactly 9 bytes of raw finger data
///
/// # Coordinate Extraction
///
/// From the C driver:
/// ```c
/// USHORT tmp_x = (*(USHORT*)f) & 0x1fff;      // 13-bit X from bytes 0-1
/// x = (SHORT)(tmp_x << 3) >> 3;                // sign-extend
/// UINT tmp_y = (INT)(*(UINT*)f);               // 32-bit from bytes 0-3
/// y = -(INT)(tmp_y << 6) >> 19;                // extract bits 13-25, negate
/// ```
#[must_use]
pub fn parse_type5_finger(data: &[u8; 9]) -> Finger {
    // Extract X: 13-bit signed value from bytes 0-1
    let raw_u16 = u16::from_le_bytes([data[0], data[1]]);
    let tmp_x = raw_u16 & 0x1fff; // mask to 13 bits
    // Sign-extend from 13 bits: shift left to put bit 12 at sign position, arithmetic shift back
    let x = ((tmp_x << 3) as i16) >> 3;

    // Extract Y: 13-bit signed value from bits 13-25 of bytes 0-3, negated
    let raw_u32 = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let y = -((raw_u32 << 6) as i32 >> 19);

    // Contact ID (lower 4 bits) and orientation (upper 4 bits) from byte 8
    let id = data[8] & 0x0f;
    let orientation = (data[8] >> 4) & 0x0f;

    Finger {
        raw_x: i32::from(x),
        raw_y: y,
        touch_major: data[4],
        touch_minor: data[5],
        size: data[6],
        pressure: data[7],
        contact_id: id,
        orientation,
    }
}

/// Extract TYPE2/3/4 finger data from a raw finger block.
///
/// These use 28-byte (TYPE2/3) or 30-byte (TYPE4) le16-aligned structures.
/// Fields are all `u16` little-endian.
///
/// # Arguments
/// * `data` — raw finger block (at least 28 bytes for TYPE2/3, 30 for TYPE4)
/// * `config` — device configuration for Y-axis inversion
///
/// # Coordinate Transformation
///
/// For TYPE2-4, Y is inverted: `ptp_y = config.y.max - raw_y`
#[must_use]
pub fn parse_legacy_finger(data: &[u8], config: &DeviceConfig) -> Finger {
    // All fields are le16
    let abs_x = u16::from_le_bytes([data[2], data[3]]);
    let abs_y = u16::from_le_bytes([data[4], data[5]]);
    let touch_major = u16::from_le_bytes([data[16], data[17]]);
    let touch_minor = u16::from_le_bytes([data[18], data[19]]);
    let pressure = u16::from_le_bytes([data[24], data[25]]);

    // raw_to_integer: interpret u16 as i16 (signed reinterpret)
    let raw_x = abs_x as i16 as i32;
    // TYPE2-4: Y axis is inverted in transform (y_max - raw_y)
    let raw_y = config.y.max - (abs_y as i16 as i32);

    Finger {
        raw_x,
        raw_y,
        touch_major: (touch_major >> 1) as u8, // scale down for consistent API
        touch_minor: (touch_minor >> 1) as u8,
        size: 0,
        pressure: (pressure >> 1) as u8,
        contact_id: 0, // TYPE2-4 don't have per-finger IDs; caller assigns index
        orientation: 0,
    }
}

/// Parse all fingers from a raw USB interrupt report.
///
/// Returns the finger count and whether the button is pressed.
///
/// # Arguments
/// * `report` — the complete raw report bytes from the device
/// * `config` — device configuration
/// * `header_size` — header size (differs between USB and BT for TYPE5)
/// * `out` — output buffer for parsed fingers (should have room for at least
///   [`PTP_MAX_CONTACT_POINTS`] entries)
///
/// [`PTP_MAX_CONTACT_POINTS`]: crate::constants::PTP_MAX_CONTACT_POINTS
pub fn parse_report(
    report: &[u8],
    config: &DeviceConfig,
    header_size: usize,
    out: &mut [Finger],
) -> Result<(usize, bool), Error> {
    let finger_size = config.trackpad_type.finger_size();
    let delta = config.trackpad_type.finger_delta();
    let button_offset = config.trackpad_type.button_offset();

    if report.len() < header_size {
        return Err(Error::BufferTooShort {
            actual: report.len(),
            expected: header_size,
        });
    }

    // Subtract both header and delta to get the actual finger data region.
    // Delta is the offset from header end to the first finger block (nonzero for TYPE4).
    let payload = report.len() - header_size;
    let finger_payload = payload.saturating_sub(delta);
    if finger_size > 0 && finger_payload % finger_size != 0 {
        return Err(Error::MalformedPayload {
            payload_len: finger_payload,
            finger_size,
        });
    }

    let raw_count = if finger_size > 0 { finger_payload / finger_size } else { 0 };
    let count = raw_count.min(out.len());

    // Parse fingers
    for i in 0..count {
        let offset = header_size + delta + i * finger_size;
        let finger_data = &report[offset..offset + finger_size];

        out[i] = match config.trackpad_type {
            crate::device::TrackpadType::Type5 => {
                let block: &[u8; 9] = finger_data.try_into().unwrap_or(&[0u8; 9]);
                parse_type5_finger(block)
            }
            _ => {
                let mut f = parse_legacy_finger(finger_data, config);
                f.contact_id = i as u8; // TYPE2-4 use index as contact ID
                f
            }
        };
    }

    // Button state
    let button = if button_offset < report.len() {
        (report[button_offset] & 1) != 0
    } else {
        false
    };

    Ok((count, button))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::PID_MAGIC_TRACKPAD2;
    use crate::device::lookup_config;

    #[test]
    fn type5_coordinate_extraction_origin() {
        // All zeros should give (0, 0) raw coordinates
        let data = [0u8; 9];
        let f = parse_type5_finger(&data);
        assert_eq!(f.raw_x, 0);
        assert_eq!(f.raw_y, 0);
        assert_eq!(f.contact_id, 0);
        assert_eq!(f.orientation, 0);
    }

    #[test]
    fn type5_coordinate_extraction_positive_x() {
        // X = 100 (0x064): set in bytes 0-1 as LE u16
        let mut data = [0u8; 9];
        let x_val: u16 = 100;
        data[0] = (x_val & 0xff) as u8;
        data[1] = ((x_val >> 8) & 0xff) as u8;
        let f = parse_type5_finger(&data);
        assert_eq!(f.raw_x, 100);
    }

    #[test]
    fn type5_coordinate_extraction_negative_x() {
        // X = -100: encode as 13-bit signed in LE u16
        // -100 in 13-bit = 0x1f9c (8092)
        let val: i16 = -100;
        let encoded = (val as u16) & 0x1fff;
        let mut data = [0u8; 9];
        data[0] = (encoded & 0xff) as u8;
        data[1] = ((encoded >> 8) & 0xff) as u8;
        let f = parse_type5_finger(&data);
        assert_eq!(f.raw_x, -100);
    }

    #[test]
    fn type5_contact_id_and_orientation() {
        let mut data = [0u8; 9];
        data[8] = 0x53; // ID = 3, Orientation = 5
        let f = parse_type5_finger(&data);
        assert_eq!(f.contact_id, 3);
        assert_eq!(f.orientation, 5);
    }

    #[test]
    fn type5_touch_fields() {
        let mut data = [0u8; 9];
        data[4] = 42;  // touch_major
        data[5] = 30;  // touch_minor
        data[6] = 15;  // size
        data[7] = 200; // pressure
        let f = parse_type5_finger(&data);
        assert_eq!(f.touch_major, 42);
        assert_eq!(f.touch_minor, 30);
        assert_eq!(f.size, 15);
        assert_eq!(f.pressure, 200);
    }

    #[test]
    fn tip_switch_nonzero_touch() {
        let f = Finger {
            raw_x: 0, raw_y: 0,
            touch_major: 10, touch_minor: 5,
            size: 0, pressure: 50,
            contact_id: 0, orientation: 0,
        };
        assert!(f.is_tip_switch());
        assert!(f.is_confident()); // 5 << 1 = 10 < 345
    }

    #[test]
    fn tip_switch_zero_touch() {
        let f = Finger {
            raw_x: 0, raw_y: 0,
            touch_major: 0, touch_minor: 0,
            size: 0, pressure: 0,
            contact_id: 0, orientation: 0,
        };
        assert!(!f.is_tip_switch());
    }

    #[test]
    fn transform_mt2_coordinates() {
        let config = lookup_config(PID_MAGIC_TRACKPAD2);
        let f = Finger {
            raw_x: 0, raw_y: 0,
            touch_major: 10, touch_minor: 5,
            size: 0, pressure: 50,
            contact_id: 0, orientation: 0,
        };
        let (x, y) = f.transform_to_ptp(config);
        // 0 - (-3678) = 3678
        assert_eq!(x, 3678);
        // 0 - (-2479) = 2479
        assert_eq!(y, 2479);
    }

    #[test]
    fn transform_clamps_negative() {
        let config = lookup_config(PID_MAGIC_TRACKPAD2);
        let f = Finger {
            raw_x: -5000, raw_y: -5000,
            touch_major: 10, touch_minor: 5,
            size: 0, pressure: 50,
            contact_id: 0, orientation: 0,
        };
        let (x, y) = f.transform_to_ptp(config);
        // -5000 - (-3678) = -1322 → clamped to 0
        assert_eq!(x, 0);
        assert_eq!(y, 0);
    }

    #[test]
    fn type4_parse_report_with_delta() {
        // TYPE4: header=46, delta=2, finger_size=30
        // A report with 2 fingers: 46 + 2 + 60 = 108 bytes
        use crate::constants::{PID_T2_7A, PTP_MAX_CONTACT_POINTS};
        let config = lookup_config(PID_T2_7A);
        let header = config.trackpad_type.header_size_usb();
        let delta = config.trackpad_type.finger_delta();
        let fsize = config.trackpad_type.finger_size();
        assert_eq!(header, 46);
        assert_eq!(delta, 2);
        assert_eq!(fsize, 30);

        // 46 + 2 + 2*30 = 108 bytes
        let mut report = [0u8; 108];

        // Set button byte (offset 31 for TYPE4) to pressed
        report[crate::constants::BUTTON_TYPE4] = 0x01;

        let mut fingers = [Finger {
            raw_x: 0, raw_y: 0,
            touch_major: 0, touch_minor: 0,
            size: 0, pressure: 0,
            contact_id: 0, orientation: 0,
        }; PTP_MAX_CONTACT_POINTS];

        let result = parse_report(&report, config, header, &mut fingers);
        assert!(result.is_ok(), "TYPE4 parse_report should succeed, got: {result:?}");
        let (count, button) = result.unwrap();
        assert_eq!(count, 2);
        assert!(button);
    }

    #[test]
    fn type5_parse_report_zero_delta() {
        // TYPE5: header=12 (USB), delta=0, finger_size=9
        // A report with 3 fingers: 12 + 0 + 27 = 39 bytes
        use crate::constants::PTP_MAX_CONTACT_POINTS;
        let config = lookup_config(PID_MAGIC_TRACKPAD2);
        let header = config.trackpad_type.header_size_usb();
        assert_eq!(config.trackpad_type.finger_delta(), 0);

        // 12 + 3*9 = 39 bytes
        let report = [0u8; 39];

        let mut fingers = [Finger {
            raw_x: 0, raw_y: 0,
            touch_major: 0, touch_minor: 0,
            size: 0, pressure: 0,
            contact_id: 0, orientation: 0,
        }; PTP_MAX_CONTACT_POINTS];

        let (count, _) = parse_report(&report, config, header, &mut fingers).unwrap();
        assert_eq!(count, 3);
    }
}
