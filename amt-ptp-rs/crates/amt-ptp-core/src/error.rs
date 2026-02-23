//! Error types for the `amt-ptp-core` library.

/// Errors that can occur during finger data parsing or report generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Input buffer is shorter than the expected header size.
    BufferTooShort {
        /// Number of bytes received.
        actual: usize,
        /// Minimum expected (header size).
        expected: usize,
    },
    /// The payload after the header is not an exact multiple of the finger size.
    MalformedPayload {
        /// Payload bytes (total - header).
        payload_len: usize,
        /// Expected finger block size.
        finger_size: usize,
    },
    /// Requested finger index is out of range.
    FingerIndexOutOfRange {
        /// Requested index.
        index: usize,
        /// Number of fingers in the report.
        count: usize,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BufferTooShort { actual, expected } => {
                write!(f, "buffer too short: {actual} bytes, need at least {expected}")
            }
            Self::MalformedPayload { payload_len, finger_size } => {
                write!(
                    f,
                    "malformed payload: {payload_len} bytes is not a multiple of finger size {finger_size}"
                )
            }
            Self::FingerIndexOutOfRange { index, count } => {
                write!(f, "finger index {index} out of range (count = {count})")
            }
        }
    }
}
