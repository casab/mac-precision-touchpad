//! Device configuration types and per-device parameter tables.
//!
//! Each supported Apple trackpad has a [`DeviceConfig`] entry that describes
//! its firmware format, coordinate ranges, USB endpoints, and Wellspring
//! mode-switch parameters. The driver looks up the connected device's PID
//! in [`CONFIG_TABLE`] to find the right configuration.
//!
//! Ported from `AppleDefinition.h` / `Bcm5974ConfigTable[]` in the C driver.

use crate::constants::*;

/// Apple trackpad firmware format version.
///
/// Determines header size, finger block layout, button offset, and
/// coordinate extraction method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrackpadType {
    /// Plain trackpad (MacBook Air 1st gen). 28-byte finger blocks.
    Type1 = 0,
    /// Button integrated in trackpad (MacBook Pro Penryn+). 28-byte finger blocks.
    Type2 = 1,
    /// Additional header fields (June 2013+). 28-byte finger blocks.
    Type3 = 2,
    /// Pressure data field added (T2 and Wellspring9). 30-byte finger blocks.
    Type4 = 3,
    /// Magic Trackpad 2 packed format. 9-byte finger blocks.
    Type5 = 4,
}

impl TrackpadType {
    /// Header size in bytes for this trackpad type (USB transport).
    #[must_use]
    pub const fn header_size_usb(self) -> usize {
        match self {
            Self::Type1 => HEADER_TYPE1,
            Self::Type2 => HEADER_TYPE2,
            Self::Type3 => HEADER_TYPE3,
            Self::Type4 => HEADER_TYPE4,
            Self::Type5 => HEADER_TYPE5_USB,
        }
    }

    /// Header size in bytes for this trackpad type (Bluetooth transport).
    /// Only differs for TYPE5; all others are the same as USB.
    #[must_use]
    pub const fn header_size_bt(self) -> usize {
        match self {
            Self::Type5 => HEADER_TYPE5_BT,
            other => other.header_size_usb(),
        }
    }

    /// Finger data block size in bytes.
    #[must_use]
    pub const fn finger_size(self) -> usize {
        match self {
            Self::Type1 => FSIZE_TYPE1,
            Self::Type2 => FSIZE_TYPE2,
            Self::Type3 => FSIZE_TYPE3,
            Self::Type4 => FSIZE_TYPE4,
            Self::Type5 => FSIZE_TYPE5,
        }
    }

    /// Offset from header end to first finger data block.
    #[must_use]
    pub const fn finger_delta(self) -> usize {
        match self {
            Self::Type1 => DELTA_TYPE1,
            Self::Type2 => DELTA_TYPE2,
            Self::Type3 => DELTA_TYPE3,
            Self::Type4 => DELTA_TYPE4,
            Self::Type5 => DELTA_TYPE5,
        }
    }

    /// Button byte offset within the report.
    #[must_use]
    pub const fn button_offset(self) -> usize {
        match self {
            Self::Type1 => BUTTON_TYPE1,
            Self::Type2 => BUTTON_TYPE2,
            Self::Type3 => BUTTON_TYPE3,
            Self::Type4 => BUTTON_TYPE4,
            Self::Type5 => BUTTON_TYPE5,
        }
    }

    /// Total data length for a full report (header + MAX_FINGERS × finger_size).
    #[must_use]
    pub const fn usb_report_size(self) -> usize {
        self.header_size_usb() + MAX_FINGERS * self.finger_size()
    }
}

/// Per-axis parameter limits (coordinate, pressure, width, orientation).
///
/// Maps to `BCM5974_PARAM` in the C driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisParams {
    /// Signal-to-noise ratio for this axis.
    pub sn_ratio: i32,
    /// Minimum raw value from the device.
    pub min: i32,
    /// Maximum raw value from the device.
    pub max: i32,
}

impl AxisParams {
    /// Span of raw values: `max - min`.
    #[must_use]
    pub const fn span(self) -> i32 {
        self.max - self.min
    }
}

/// USB control message parameters for Wellspring mode switching.
///
/// Maps to the `USBMSG_TYPEn` macros in the C driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WellspringMsg {
    /// Control transfer data length.
    pub size: u16,
    /// wValue for the USB control transfer.
    pub req_val: u16,
    /// wIndex for the USB control transfer.
    pub req_idx: u16,
    /// Index into the control buffer for the mode switch byte.
    pub switch_idx: u16,
    /// Value to write for mode ON (raw multitouch data).
    pub switch_on: u8,
    /// Value to write for mode OFF (standard HID reports).
    pub switch_off: u8,
}

/// USB Wellspring parameters for TYPE1-3 firmware.
pub const WELLSPRING_MSG_TYPE1: WellspringMsg = WellspringMsg {
    size: 8,
    req_val: 0x300,
    req_idx: 0,
    switch_idx: 0,
    switch_on: 0x1,
    switch_off: 0x8,
};

/// USB Wellspring parameters for TYPE4 firmware.
pub const WELLSPRING_MSG_TYPE4: WellspringMsg = WellspringMsg {
    size: 2,
    req_val: 0x302,
    req_idx: 2,
    switch_idx: 1,
    switch_on: 0x1,
    switch_off: 0x0,
};

/// USB Wellspring parameters for TYPE5 (Magic Trackpad 2) firmware.
pub const WELLSPRING_MSG_TYPE5: WellspringMsg = WellspringMsg {
    size: 2,
    req_val: 0x302,
    req_idx: 1,
    switch_idx: 1,
    switch_on: 0x1,
    switch_off: 0x0,
};

/// Device capability flags.
pub const HAS_INTEGRATED_BUTTON: u32 = 1;

/// Complete configuration for one Apple trackpad device.
///
/// Maps to `BCM5974_CONFIG` in the C driver. Each supported PID has an entry
/// in [`CONFIG_TABLE`].
#[derive(Debug, Clone, Copy)]
pub struct DeviceConfig {
    /// USB product ID (or `PID_DEFAULT_FALLBACK` for the catch-all entry).
    pub product_id: u16,
    /// Capability bitmask (see [`HAS_INTEGRATED_BUTTON`]).
    pub caps: u32,
    /// Button endpoint address (0 if button data is inline in trackpad report).
    pub button_endpoint: u8,
    /// Button data length in bytes.
    pub button_data_len: usize,
    /// Trackpad interrupt endpoint address.
    pub trackpad_endpoint: u8,
    /// Firmware format type.
    pub trackpad_type: TrackpadType,
    /// Wellspring USB control message parameters.
    pub wellspring_msg: WellspringMsg,
    /// Finger pressure limits.
    pub pressure: AxisParams,
    /// Finger width limits.
    pub width: AxisParams,
    /// Horizontal (X) coordinate limits.
    pub x: AxisParams,
    /// Vertical (Y) coordinate limits.
    pub y: AxisParams,
    /// Orientation limits.
    pub orientation: AxisParams,
}

impl DeviceConfig {
    /// Whether the device has an integrated clickpad button.
    #[must_use]
    pub const fn has_integrated_button(&self) -> bool {
        self.caps & HAS_INTEGRATED_BUTTON != 0
    }

    /// PTP logical maximum for X axis (raw span = x.max - x.min).
    #[must_use]
    pub const fn ptp_x_logical_max(&self) -> u16 {
        (self.x.max - self.x.min) as u16
    }

    /// PTP logical maximum for Y axis (raw span = y.max - y.min).
    #[must_use]
    pub const fn ptp_y_logical_max(&self) -> u16 {
        (self.y.max - self.y.min) as u16
    }
}

/// Device configuration table, searched by product ID.
///
/// The first entry is the fallback (oversampled ranges) for unknown T2 devices.
/// The last entry is a sentinel with `product_id = 0`.
///
/// Ported from `Bcm5974ConfigTable[]` in the USB KM driver's `AppleDefinition.h`.
pub static CONFIG_TABLE: &[DeviceConfig] = &[
    // Fallback for unknown T2 devices (oversampled ranges)
    DeviceConfig {
        product_id: PID_DEFAULT_FALLBACK,
        caps: HAS_INTEGRATED_BUTTON,
        button_endpoint: 0,
        button_data_len: 4, // sizeof(TRACKPAD_BUTTON_DATA)
        trackpad_endpoint: 0x83,
        trackpad_type: TrackpadType::Type4,
        wellspring_msg: WELLSPRING_MSG_TYPE4,
        pressure: AxisParams { sn_ratio: SN_PRESSURE, min: 0, max: 300 },
        width: AxisParams { sn_ratio: SN_WIDTH, min: 0, max: 2048 },
        x: AxisParams { sn_ratio: SN_COORD, min: -10000, max: 10000 },
        y: AxisParams { sn_ratio: SN_COORD, min: -2000, max: 10000 },
        orientation: AxisParams { sn_ratio: SN_ORIENT, min: -MAX_FINGER_ORIENTATION, max: MAX_FINGER_ORIENTATION },
    },
    // MacBook Pro 13″ 2018 (T2)
    DeviceConfig {
        product_id: PID_T2_7A,
        caps: HAS_INTEGRATED_BUTTON,
        button_endpoint: 0,
        button_data_len: 4,
        trackpad_endpoint: 0x83,
        trackpad_type: TrackpadType::Type4,
        wellspring_msg: WELLSPRING_MSG_TYPE4,
        pressure: AxisParams { sn_ratio: SN_PRESSURE, min: 0, max: 300 },
        width: AxisParams { sn_ratio: SN_WIDTH, min: 0, max: 2048 },
        x: AxisParams { sn_ratio: SN_COORD, min: -6243, max: 6749 },
        y: AxisParams { sn_ratio: SN_COORD, min: -170, max: 7685 },
        orientation: AxisParams { sn_ratio: SN_ORIENT, min: -MAX_FINGER_ORIENTATION, max: MAX_FINGER_ORIENTATION },
    },
    // MacBook Pro 13″ 2019 (T2)
    DeviceConfig {
        product_id: PID_T2_7B,
        caps: HAS_INTEGRATED_BUTTON,
        button_endpoint: 0,
        button_data_len: 4,
        trackpad_endpoint: 0x83,
        trackpad_type: TrackpadType::Type4,
        wellspring_msg: WELLSPRING_MSG_TYPE4,
        pressure: AxisParams { sn_ratio: SN_PRESSURE, min: 0, max: 300 },
        width: AxisParams { sn_ratio: SN_WIDTH, min: 0, max: 2048 },
        x: AxisParams { sn_ratio: SN_COORD, min: -6243, max: 6749 },
        y: AxisParams { sn_ratio: SN_COORD, min: -170, max: 7685 },
        orientation: AxisParams { sn_ratio: SN_ORIENT, min: -MAX_FINGER_ORIENTATION, max: MAX_FINGER_ORIENTATION },
    },
    // MacBook Pro 15″ 2018 (T2, oversampled)
    DeviceConfig {
        product_id: PID_T2_7C,
        caps: HAS_INTEGRATED_BUTTON,
        button_endpoint: 0,
        button_data_len: 4,
        trackpad_endpoint: 0x83,
        trackpad_type: TrackpadType::Type4,
        wellspring_msg: WELLSPRING_MSG_TYPE4,
        pressure: AxisParams { sn_ratio: SN_PRESSURE, min: 0, max: 300 },
        width: AxisParams { sn_ratio: SN_WIDTH, min: 0, max: 2048 },
        x: AxisParams { sn_ratio: SN_COORD, min: -10000, max: 10000 },
        y: AxisParams { sn_ratio: SN_COORD, min: -2000, max: 10000 },
        orientation: AxisParams { sn_ratio: SN_ORIENT, min: -MAX_FINGER_ORIENTATION, max: MAX_FINGER_ORIENTATION },
    },
    // MacBook Pro 15″ 2019 (T2, oversampled)
    DeviceConfig {
        product_id: PID_T2_7D,
        caps: HAS_INTEGRATED_BUTTON,
        button_endpoint: 0,
        button_data_len: 4,
        trackpad_endpoint: 0x83,
        trackpad_type: TrackpadType::Type4,
        wellspring_msg: WELLSPRING_MSG_TYPE4,
        pressure: AxisParams { sn_ratio: SN_PRESSURE, min: 0, max: 300 },
        width: AxisParams { sn_ratio: SN_WIDTH, min: 0, max: 2048 },
        x: AxisParams { sn_ratio: SN_COORD, min: -10000, max: 10000 },
        y: AxisParams { sn_ratio: SN_COORD, min: -2000, max: 10000 },
        orientation: AxisParams { sn_ratio: SN_ORIENT, min: -MAX_FINGER_ORIENTATION, max: MAX_FINGER_ORIENTATION },
    },
    // Magic Trackpad 2 (USB and BT, TYPE5)
    DeviceConfig {
        product_id: PID_MAGIC_TRACKPAD2,
        caps: HAS_INTEGRATED_BUTTON,
        button_endpoint: 0,
        button_data_len: 4,
        trackpad_endpoint: 0x83,
        trackpad_type: TrackpadType::Type5,
        wellspring_msg: WELLSPRING_MSG_TYPE5,
        pressure: AxisParams { sn_ratio: SN_PRESSURE, min: 0, max: 300 },
        width: AxisParams { sn_ratio: SN_WIDTH, min: 0, max: 2048 },
        x: AxisParams { sn_ratio: SN_COORD, min: -3678, max: 3934 },
        y: AxisParams { sn_ratio: SN_COORD, min: -2479, max: 2586 },
        orientation: AxisParams { sn_ratio: SN_ORIENT, min: -MAX_FINGER_ORIENTATION, max: MAX_FINGER_ORIENTATION },
    },
];

/// Look up a device configuration by USB product ID.
///
/// Returns the matching config, or the fallback entry (`PID_DEFAULT_FALLBACK`)
/// if no exact match is found.
#[must_use]
pub fn lookup_config(product_id: u16) -> &'static DeviceConfig {
    CONFIG_TABLE
        .iter()
        .find(|c| c.product_id == product_id)
        .unwrap_or(&CONFIG_TABLE[0]) // fallback entry is first
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_magic_trackpad2() {
        let cfg = lookup_config(PID_MAGIC_TRACKPAD2);
        assert_eq!(cfg.product_id, 0x0265);
        assert!(matches!(cfg.trackpad_type, TrackpadType::Type5));
        assert_eq!(cfg.trackpad_type.finger_size(), 9);
        assert_eq!(cfg.trackpad_type.header_size_usb(), 12);
        assert_eq!(cfg.trackpad_type.header_size_bt(), 4);
        assert!(cfg.has_integrated_button());
    }

    #[test]
    fn lookup_t2_13inch() {
        let cfg = lookup_config(PID_T2_7A);
        assert_eq!(cfg.product_id, 0x027a);
        assert!(matches!(cfg.trackpad_type, TrackpadType::Type4));
        assert_eq!(cfg.trackpad_type.finger_size(), 30);
        assert_eq!(cfg.x.min, -6243);
        assert_eq!(cfg.x.max, 6749);
    }

    #[test]
    fn lookup_unknown_falls_back() {
        let cfg = lookup_config(0x9999);
        assert_eq!(cfg.product_id, PID_DEFAULT_FALLBACK);
        assert!(matches!(cfg.trackpad_type, TrackpadType::Type4));
    }

    #[test]
    fn ptp_logical_max_mt2() {
        let cfg = lookup_config(PID_MAGIC_TRACKPAD2);
        // X span: 3934 - (-3678) = 7612
        assert_eq!(cfg.ptp_x_logical_max(), 7612);
        // Y span: 2586 - (-2479) = 5065
        assert_eq!(cfg.ptp_y_logical_max(), 5065);
    }

    #[test]
    fn type5_report_size() {
        let tp = TrackpadType::Type5;
        // 12 + 16 × 9 = 156
        assert_eq!(tp.usb_report_size(), 12 + 16 * 9);
    }

    #[test]
    fn type4_report_size() {
        let tp = TrackpadType::Type4;
        // 46 + 16 × 30 = 526
        assert_eq!(tp.usb_report_size(), 46 + 16 * 30);
    }
}
