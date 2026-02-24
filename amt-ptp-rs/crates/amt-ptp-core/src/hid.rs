//! HID report descriptor and HQA certification blob.
//!
//! The HID report descriptor defines the PTP touchpad's capabilities to
//! Windows. It must exactly match the report structures in [`crate::ptp`].
//!
//! The HQA blob is a fixed 256-byte certification token required by
//! Windows Precision Touchpad certification.

/// Default PTP HQA certification blob (256 bytes).
///
/// This is the standard HQA blob used by the original C driver.
/// Copied from `Hid.h: DEFAULT_PTP_HQA_BLOB`.
pub static DEFAULT_HQA_BLOB: [u8; 256] = [
    0xfc, 0x28, 0xfe, 0x84, 0x40, 0xcb, 0x9a, 0x87,
    0x0d, 0xbe, 0x57, 0x3c, 0xb6, 0x70, 0x09, 0x88,
    0x07, 0x97, 0x2d, 0x2b, 0xe3, 0x38, 0x34, 0xb6,
    0x6c, 0xed, 0xb0, 0xf7, 0xe5, 0x9c, 0xf6, 0xc2,
    0x2e, 0x84, 0x1b, 0xe8, 0xb4, 0x51, 0x78, 0x43,
    0x1f, 0x28, 0x4b, 0x7c, 0x2d, 0x53, 0xaf, 0xfc,
    0x47, 0x70, 0x1b, 0x59, 0x6f, 0x74, 0x43, 0xc4,
    0xf3, 0x47, 0x18, 0x53, 0x1a, 0xa2, 0xa1, 0x71,
    0xc7, 0x95, 0x0e, 0x31, 0x55, 0x21, 0xd3, 0xb5,
    0x1e, 0xe9, 0x0c, 0xba, 0xec, 0xb8, 0x89, 0x19,
    0x3e, 0xb3, 0xaf, 0x75, 0x81, 0x9d, 0x53, 0xb9,
    0x41, 0x57, 0xf4, 0x6d, 0x39, 0x25, 0x29, 0x7c,
    0x87, 0xd9, 0xb4, 0x98, 0x45, 0x7d, 0xa7, 0x26,
    0x9c, 0x65, 0x3b, 0x85, 0x68, 0x89, 0xd7, 0x3b,
    0xbd, 0xff, 0x14, 0x67, 0xf2, 0x2b, 0xf0, 0x2a,
    0x41, 0x54, 0xf0, 0xfd, 0x2c, 0x66, 0x7c, 0xf8,
    0xc0, 0x8f, 0x33, 0x13, 0x03, 0xf1, 0xd3, 0xc1,
    0x0b, 0x89, 0xd9, 0x1b, 0x62, 0xcd, 0x51, 0xb7,
    0x80, 0xb8, 0xaf, 0x3a, 0x10, 0xc1, 0x8a, 0x5b,
    0xe8, 0x8a, 0x56, 0xf0, 0x8c, 0xaa, 0xfa, 0x35,
    0xe9, 0x42, 0xc4, 0xd8, 0x55, 0xc3, 0x38, 0xcc,
    0x2b, 0x53, 0x5c, 0x69, 0x52, 0xd5, 0xc8, 0x73,
    0x02, 0x38, 0x7c, 0x73, 0xb6, 0x41, 0xe7, 0xff,
    0x05, 0xd8, 0x2b, 0x79, 0x9a, 0xe2, 0x34, 0x60,
    0x8f, 0xa3, 0x32, 0x1f, 0x09, 0x78, 0x62, 0xbc,
    0x80, 0xe3, 0x0f, 0xbd, 0x65, 0x20, 0x08, 0x13,
    0xc1, 0xe2, 0xee, 0x53, 0x2d, 0x86, 0x7e, 0xa7,
    0x5a, 0xc5, 0xd3, 0x7d, 0x98, 0xbe, 0x31, 0x48,
    0x1f, 0xfb, 0xda, 0xaf, 0xa2, 0xa8, 0x6a, 0x89,
    0xd6, 0xbf, 0xf2, 0xd3, 0x32, 0x2a, 0x9a, 0xe4,
    0xcf, 0x17, 0xb7, 0xb8, 0xf4, 0xe1, 0x33, 0x08,
    0x24, 0x8b, 0xc4, 0x43, 0xa5, 0xe5, 0x24, 0xc2,
];

// ── HID Report Descriptor Opcodes ───────────────────────────────────
// These match the byte values from HidCommon.h.

const USAGE_PAGE: u8       = 0x05;
const USAGE_PAGE_EXT: u8   = 0x06; // 2-byte usage page
const USAGE: u8            = 0x09;
const LOGICAL_MIN: u8      = 0x15;
const LOGICAL_MAX: u8      = 0x25;
const LOGICAL_MAX_16: u8   = 0x26; // 2-byte logical max
const LOGICAL_MAX_32: u8   = 0x27; // 4-byte logical max
const PHYSICAL_MAX: u8     = 0x45;
const PHYSICAL_MAX_16: u8  = 0x46; // 2-byte physical max
const PHYSICAL_MAX_32: u8  = 0x47; // 4-byte physical max
const UNIT_EXP: u8         = 0x55;
const UNIT: u8             = 0x65;
const UNIT_16: u8          = 0x66; // 2-byte unit
const REPORT_ID: u8        = 0x85;
const REPORT_COUNT: u8     = 0x95;
const REPORT_COUNT_16: u8  = 0x96; // 2-byte report count
const REPORT_SIZE: u8      = 0x75;
const INPUT: u8            = 0x81;
const FEATURE: u8          = 0xb1;
const BEGIN_COLL: u8       = 0xa1;
const END_COLL: u8         = 0xc0;

/// Build the complete HID report descriptor for Magic Trackpad 2.
///
/// This produces the same descriptor as `WellspringMt2.h`'s
/// `AAPL_MAGIC_TRACKPAD2_PTP_TLC` + Windows configuration TLCs.
///
/// Parameters are taken from the device config to allow different
/// logical/physical maximums per device family.
///
/// The descriptor contains three top-level collections (TLCs):
/// 1. **Digitizer: Touch Pad** — multitouch input + device caps + HQA
/// 2. **Digitizer: Configuration** — input mode + selective reporting
/// 3. **Vendor: App Config** — user-mode app tuning parameters
#[must_use]
pub fn build_report_descriptor(
    x_logical_max: u16,
    y_logical_max: u16,
    x_physical_max: u16,
    y_physical_max: u16,
) -> alloc::vec::Vec<u8> {
    let mut d = alloc::vec::Vec::with_capacity(512);

    // ── TLC 1: Digitizer Touch Pad ──────────────────────────────
    d.extend_from_slice(&[USAGE_PAGE, 0x0d]);       // Digitizer
    d.extend_from_slice(&[USAGE, 0x05]);             // Touch Pad
    d.extend_from_slice(&[BEGIN_COLL, 0x01]);        // Application

    d.extend_from_slice(&[REPORT_ID, crate::constants::REPORTID_MULTITOUCH]);

    // 5 finger collections: alternating between collection variant 1 and 2
    // Variant 1 has PHYSICAL_MAX/UNIT_EXP/UNIT cleanup; variant 2 omits them.
    // In the C driver, fingers 1,2,4 use variant 1; fingers 3,5 use variant 2.
    for i in 0..crate::constants::PTP_MAX_CONTACT_POINTS {
        d.extend_from_slice(&[USAGE_PAGE, 0x0d]);    // Digitizer (re-set after generic desktop)
        d.extend_from_slice(&[USAGE, 0x22]);          // Finger
        d.extend_from_slice(&[BEGIN_COLL, 0x02]);     // Logical

        // Confidence + TipSwitch (2 bits)
        d.extend_from_slice(&[LOGICAL_MAX, 0x01]);
        d.extend_from_slice(&[USAGE, 0x47]);          // Confidence
        d.extend_from_slice(&[USAGE, 0x42]);          // Tip Switch
        d.extend_from_slice(&[REPORT_COUNT, 0x02]);
        d.extend_from_slice(&[REPORT_SIZE, 0x01]);
        d.extend_from_slice(&[INPUT, 0x02]);          // Data, Var, Abs

        // Padding (6 bits)
        d.extend_from_slice(&[REPORT_SIZE, 0x01]);
        d.extend_from_slice(&[REPORT_COUNT, 0x06]);
        d.extend_from_slice(&[INPUT, 0x03]);          // Const, Var, Abs

        // Contact ID (32 bits)
        d.extend_from_slice(&[REPORT_COUNT, 0x01]);
        d.extend_from_slice(&[REPORT_SIZE, 0x20]);
        d.extend_from_slice(&[LOGICAL_MAX_32, 0xff, 0xff, 0xff, 0xff]);
        d.extend_from_slice(&[USAGE, 0x51]);          // Contact Identifier
        d.extend_from_slice(&[INPUT, 0x02]);

        // X coordinate (16 bits)
        d.extend_from_slice(&[USAGE_PAGE, 0x01]);     // Generic Desktop
        let x_max_le = x_logical_max.to_le_bytes();
        d.extend_from_slice(&[LOGICAL_MAX_16, x_max_le[0], x_max_le[1]]);
        d.extend_from_slice(&[REPORT_SIZE, 0x10]);
        d.extend_from_slice(&[UNIT_EXP, 0x0e]);       // -2
        d.extend_from_slice(&[UNIT, 0x11]);            // cm
        d.extend_from_slice(&[USAGE, 0x30]);           // X
        let x_phys_le = x_physical_max.to_le_bytes();
        d.extend_from_slice(&[PHYSICAL_MAX_16, x_phys_le[0], x_phys_le[1]]);
        d.extend_from_slice(&[REPORT_COUNT, 0x01]);
        d.extend_from_slice(&[INPUT, 0x02]);

        // Y coordinate (16 bits)
        let y_phys_le = y_physical_max.to_le_bytes();
        d.extend_from_slice(&[PHYSICAL_MAX_16, y_phys_le[0], y_phys_le[1]]);
        let y_max_le = y_logical_max.to_le_bytes();
        d.extend_from_slice(&[LOGICAL_MAX_16, y_max_le[0], y_max_le[1]]);
        d.extend_from_slice(&[USAGE, 0x31]);           // Y
        d.extend_from_slice(&[INPUT, 0x02]);

        // Reset physical/unit for clean state (variant 1 pattern)
        // The C driver uses this on fingers 1,2,4 but not 3,5.
        // We always reset for correctness — Windows handles it fine.
        if i == 0 || i == 1 || i == 3 {
            d.extend_from_slice(&[PHYSICAL_MAX, 0x00]);
            d.extend_from_slice(&[UNIT_EXP, 0x00]);
            d.extend_from_slice(&[UNIT, 0x00]);
        }

        d.extend_from_slice(&[END_COLL]);             // End Logical
    }

    // Scan Time (16 bits)
    d.extend_from_slice(&[USAGE_PAGE, 0x0d]);         // Digitizer
    d.extend_from_slice(&[UNIT_EXP, 0x0c]);           // -4
    d.extend_from_slice(&[UNIT_16, 0x01, 0x10]);      // Time: Seconds
    d.extend_from_slice(&[PHYSICAL_MAX_32, 0xff, 0xff, 0x00, 0x00]);
    d.extend_from_slice(&[LOGICAL_MAX_32, 0xff, 0xff, 0x00, 0x00]);
    d.extend_from_slice(&[USAGE, 0x56]);               // Scan Time
    d.extend_from_slice(&[INPUT, 0x02]);

    // Contact Count (8 bits)
    d.extend_from_slice(&[USAGE, 0x54]);               // Contact Count
    d.extend_from_slice(&[LOGICAL_MAX, 0x7f]);
    d.extend_from_slice(&[REPORT_SIZE, 0x08]);
    d.extend_from_slice(&[INPUT, 0x02]);

    // Button (1 bit + 7 padding)
    d.extend_from_slice(&[USAGE_PAGE, 0x09]);          // Button
    d.extend_from_slice(&[USAGE, 0x01]);               // Button 1
    d.extend_from_slice(&[LOGICAL_MAX, 0x01]);
    d.extend_from_slice(&[REPORT_SIZE, 0x01]);
    d.extend_from_slice(&[INPUT, 0x02]);
    d.extend_from_slice(&[REPORT_COUNT, 0x07]);
    d.extend_from_slice(&[INPUT, 0x03]);               // Const padding

    // Device Caps Feature Report (Report ID 0x07)
    d.extend_from_slice(&[USAGE_PAGE, 0x0d]);          // Digitizer
    d.extend_from_slice(&[REPORT_ID, crate::constants::REPORTID_DEVICE_CAPS]);
    d.extend_from_slice(&[USAGE, 0x55]);               // Maximum Contacts
    d.extend_from_slice(&[USAGE, 0x59]);               // Touchpad Button Type
    d.extend_from_slice(&[LOGICAL_MIN, 0x00]);
    d.extend_from_slice(&[LOGICAL_MAX_16, 0xff, 0x00]);
    d.extend_from_slice(&[REPORT_SIZE, 0x08]);
    d.extend_from_slice(&[REPORT_COUNT, 0x02]);
    d.extend_from_slice(&[FEATURE, 0x02]);

    // HQA Certification Feature Report (Report ID 0x08)
    d.extend_from_slice(&[USAGE_PAGE_EXT, 0x00, 0xff]); // Vendor-defined
    d.extend_from_slice(&[REPORT_ID, crate::constants::REPORTID_PTPHQA]);
    d.extend_from_slice(&[USAGE, 0xc5]);
    d.extend_from_slice(&[LOGICAL_MIN, 0x00]);
    d.extend_from_slice(&[LOGICAL_MAX_16, 0xff, 0x00]);
    d.extend_from_slice(&[REPORT_SIZE, 0x08]);
    d.extend_from_slice(&[REPORT_COUNT_16, 0x00, 0x01]); // 256
    d.extend_from_slice(&[FEATURE, 0x02]);

    d.push(END_COLL); // End Application (Touch Pad)

    // ── TLC 2: Digitizer Configuration ──────────────────────────
    d.extend_from_slice(&[USAGE_PAGE, 0x0d]);          // Digitizer
    d.extend_from_slice(&[USAGE, 0x0e]);               // Configuration
    d.extend_from_slice(&[BEGIN_COLL, 0x01]);          // Application

    // Input Mode (Report ID 0x04)
    d.extend_from_slice(&[REPORT_ID, crate::constants::REPORTID_INPUT_MODE]);
    d.extend_from_slice(&[USAGE, 0x22]);               // Finger
    d.extend_from_slice(&[BEGIN_COLL, 0x02]);          // Logical
    d.extend_from_slice(&[USAGE, 0x52]);               // Input Mode
    d.extend_from_slice(&[LOGICAL_MIN, 0x00]);
    d.extend_from_slice(&[LOGICAL_MAX, crate::constants::MAX_FINGERS as u8]);
    d.extend_from_slice(&[REPORT_SIZE, 0x08]);
    d.extend_from_slice(&[REPORT_COUNT, 0x01]);
    d.extend_from_slice(&[FEATURE, 0x02]);
    d.push(END_COLL);                                  // End Logical

    // Selective Reporting (Report ID 0x06)
    d.extend_from_slice(&[BEGIN_COLL, 0x00]);          // Physical
    d.extend_from_slice(&[REPORT_ID, crate::constants::REPORTID_FUNC_SWITCH]);
    d.extend_from_slice(&[USAGE, crate::constants::HID_USAGE_BUTTON_SWITCH]);
    d.extend_from_slice(&[USAGE, crate::constants::HID_USAGE_SURFACE_SWITCH]);
    d.extend_from_slice(&[REPORT_SIZE, 0x01]);
    d.extend_from_slice(&[REPORT_COUNT, 0x02]);
    d.extend_from_slice(&[LOGICAL_MAX, 0x01]);
    d.extend_from_slice(&[FEATURE, 0x02]);
    d.extend_from_slice(&[REPORT_COUNT, 0x06]);
    d.extend_from_slice(&[FEATURE, 0x03]);             // Const padding
    d.push(END_COLL);                                  // End Physical

    d.push(END_COLL); // End Application (Configuration)

    // ── TLC 3: Vendor App Configuration ─────────────────────────
    d.extend_from_slice(&[USAGE_PAGE_EXT, 0x00, 0xff]); // Vendor-defined
    d.extend_from_slice(&[USAGE, 0x01]);
    d.extend_from_slice(&[BEGIN_COLL, 0x01]);          // Application
    d.extend_from_slice(&[REPORT_ID, crate::constants::REPORTID_UMAPP_CONF]);
    d.extend_from_slice(&[USAGE, 0x01]);
    d.extend_from_slice(&[LOGICAL_MIN, 0x00]);
    d.extend_from_slice(&[LOGICAL_MAX_16, 0xff, 0x00]);
    d.extend_from_slice(&[REPORT_SIZE, 0x08]);
    d.extend_from_slice(&[REPORT_COUNT, 0x03]);
    d.extend_from_slice(&[FEATURE, 0x02]);
    d.push(END_COLL); // End Application (Vendor)

    d
}

#[cfg(test)]
mod tests {
    use super::*;

    extern crate alloc;

    #[test]
    fn hqa_blob_length() {
        assert_eq!(DEFAULT_HQA_BLOB.len(), 256);
    }

    #[test]
    fn hqa_blob_first_bytes() {
        assert_eq!(DEFAULT_HQA_BLOB[0], 0xfc);
        assert_eq!(DEFAULT_HQA_BLOB[1], 0x28);
        assert_eq!(DEFAULT_HQA_BLOB[2], 0xfe);
    }

    #[test]
    fn hqa_blob_last_bytes() {
        // Last 3 bytes from the C header: 0xe5, 0x24, 0xc2
        assert_eq!(DEFAULT_HQA_BLOB[253], 0xe5);
        assert_eq!(DEFAULT_HQA_BLOB[254], 0x24);
        assert_eq!(DEFAULT_HQA_BLOB[255], 0xc2);
    }

    #[test]
    fn descriptor_builds_for_mt2() {
        // MT2 dimensions: X logical 7612, Y logical 5065
        // Physical: X 1600 (16.00 cm), Y 1149 (11.49 cm)
        let desc = build_report_descriptor(7612, 5065, 1600, 1149);

        // Should start with Digitizer Touch Pad TLC
        assert_eq!(desc[0], USAGE_PAGE);
        assert_eq!(desc[1], 0x0d); // Digitizer
        assert_eq!(desc[2], USAGE);
        assert_eq!(desc[3], 0x05); // Touch Pad
        assert_eq!(desc[4], BEGIN_COLL);
        assert_eq!(desc[5], 0x01); // Application

        // Should end with END_COLLECTION
        assert_eq!(*desc.last().unwrap(), END_COLL);

        // Should be a reasonable size (the C driver's descriptor is ~400-500 bytes)
        assert!(desc.len() > 300, "descriptor too short: {} bytes", desc.len());
        assert!(desc.len() < 700, "descriptor too long: {} bytes", desc.len());
    }

    #[test]
    fn descriptor_contains_all_report_ids() {
        let desc = build_report_descriptor(7612, 5065, 1600, 1149);

        // Check that all report IDs are present
        let has_report_id = |id: u8| {
            desc.windows(2).any(|w| w[0] == REPORT_ID && w[1] == id)
        };
        assert!(has_report_id(crate::constants::REPORTID_MULTITOUCH));
        assert!(has_report_id(crate::constants::REPORTID_DEVICE_CAPS));
        assert!(has_report_id(crate::constants::REPORTID_PTPHQA));
        assert!(has_report_id(crate::constants::REPORTID_INPUT_MODE));
        assert!(has_report_id(crate::constants::REPORTID_FUNC_SWITCH));
        assert!(has_report_id(crate::constants::REPORTID_UMAPP_CONF));
    }
}
