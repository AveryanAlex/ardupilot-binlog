use std::sync::Arc;

use crate::error::BinlogError;
use crate::value::FieldValue;

/// Schema definition for a DataFlash message type, parsed from a FMT message.
#[derive(Debug, Clone)]
pub struct MessageFormat {
    /// Message type ID (0–255)
    pub msg_type: u8,
    /// Total message length in bytes (including 3-byte header)
    pub msg_len: u8,
    /// Message name (e.g. "ATT", "GPS", "IMU")
    pub name: String,
    /// Raw format string (e.g. "QccccCCCC")
    pub format: String,
    /// Field labels in order (e.g. ["TimeUS", "Roll", "Pitch", ...]).
    /// Shared with parsed entries via `Arc` to avoid per-entry string copies.
    pub labels: Arc<[String]>,
}

/// Return the byte size of a single format character.
fn field_size(c: char) -> Result<usize, BinlogError> {
    match c {
        'b' | 'B' | 'M' => Ok(1),
        'h' | 'H' | 'c' | 'C' => Ok(2),
        'i' | 'I' | 'e' | 'E' | 'f' | 'L' | 'n' => Ok(4),
        'q' | 'Q' | 'd' => Ok(8),
        'N' => Ok(16),
        'a' | 'Z' => Ok(64),
        _ => Err(BinlogError::InvalidFormat(c)),
    }
}

/// Decode a fixed-size null-padded byte slice into a trimmed String.
fn decode_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end])
        .trim_end()
        .to_string()
}

impl MessageFormat {
    /// Return the computed payload size from the format string (sum of field sizes).
    #[must_use]
    pub fn payload_size(&self) -> usize {
        self.format.chars().filter_map(|c| field_size(c).ok()).sum()
    }

    /// Extract a microsecond timestamp from the raw payload bytes, if present.
    ///
    /// - Format char `'Q'`: first 8 bytes as `u64` (already microseconds).
    /// - Format char `'I'` with first label `"TimeMS"` or `"TimeUS"`: first 4 bytes
    ///   as `u32`, multiplied by 1000 to convert milliseconds → microseconds.
    /// - Otherwise returns `None`.
    pub(crate) fn extract_timestamp(&self, payload: &[u8]) -> Option<u64> {
        match self.format.as_bytes().first().copied() {
            Some(b'Q') if payload.len() >= 8 => {
                let bytes: [u8; 8] = payload[..8].try_into().ok()?;
                Some(u64::from_le_bytes(bytes))
            }
            Some(b'I') if payload.len() >= 4 => {
                let is_time_label = self
                    .labels
                    .first()
                    .map(|l| l == "TimeMS" || l == "TimeUS")
                    .unwrap_or(false);
                if is_time_label {
                    let bytes: [u8; 4] = payload[..4].try_into().ok()?;
                    Some(u32::from_le_bytes(bytes) as u64 * 1000)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Decode a raw payload buffer into field values using this format's type string.
    /// Labels are shared separately via `Arc<[String]>`.
    pub fn decode_fields(&self, payload: &[u8]) -> Result<Vec<FieldValue>, BinlogError> {
        let mut values = Vec::new();
        let mut offset = 0;

        for c in self.format.chars() {
            let size = field_size(c)?;
            if offset + size > payload.len() {
                return Err(BinlogError::UnexpectedEof);
            }
            let bytes = &payload[offset..offset + size];
            values.push(decode_field(c, bytes)?);
            offset += size;
        }

        Ok(values)
    }
}

/// Convert a byte slice prefix to a fixed-size array, returning an error if too short.
fn to_array<const N: usize>(bytes: &[u8]) -> Result<[u8; N], BinlogError> {
    bytes
        .get(..N)
        .and_then(|s| s.try_into().ok())
        .ok_or(BinlogError::PayloadTooShort)
}

/// Decode a single field from its raw bytes given the format character.
fn decode_field(c: char, bytes: &[u8]) -> Result<FieldValue, BinlogError> {
    let scaled = |raw: f64| FieldValue::Float(raw / 100.0);

    match c {
        'b' => Ok(FieldValue::Int(bytes[0] as i8 as i64)),
        'B' | 'M' => Ok(FieldValue::Int(bytes[0] as i64)),
        'h' | 'H' => {
            let pair = [bytes[0], bytes[1]];
            Ok(FieldValue::Int(if c == 'h' {
                i16::from_le_bytes(pair) as i64
            } else {
                u16::from_le_bytes(pair) as i64
            }))
        }
        'i' | 'I' | 'L' => Ok(FieldValue::Int(if c == 'I' {
            u32::from_le_bytes(to_array(bytes)?) as i64
        } else {
            i32::from_le_bytes(to_array(bytes)?) as i64
        })),
        'q' => Ok(FieldValue::Int(i64::from_le_bytes(to_array(bytes)?))),
        'Q' => Ok(FieldValue::Uint(u64::from_le_bytes(to_array(bytes)?))),
        'f' => Ok(FieldValue::Float(
            f32::from_le_bytes(to_array(bytes)?) as f64
        )),
        'd' => Ok(FieldValue::Float(f64::from_le_bytes(to_array(bytes)?))),
        'c' | 'e' => Ok(scaled(if c == 'c' {
            i16::from_le_bytes([bytes[0], bytes[1]]) as f64
        } else {
            i32::from_le_bytes(to_array(bytes)?) as f64
        })),
        'C' | 'E' => Ok(scaled(if c == 'C' {
            u16::from_le_bytes([bytes[0], bytes[1]]) as f64
        } else {
            u32::from_le_bytes(to_array(bytes)?) as f64
        })),
        'n' | 'N' | 'Z' => Ok(FieldValue::String(decode_string(bytes))),
        'a' => {
            let arr = bytes
                .chunks_exact(2)
                .take(32)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            Ok(FieldValue::Array(arr))
        }
        _ => Err(BinlogError::InvalidFormat(c)),
    }
}

/// Parse an 86-byte FMT payload into a MessageFormat.
pub(crate) fn parse_fmt_payload(payload: &[u8]) -> Result<MessageFormat, BinlogError> {
    if payload.len() < 86 {
        return Err(BinlogError::UnexpectedEof);
    }

    let msg_type = payload[0];
    let msg_len = payload[1];
    let name = decode_string(&payload[2..6]);
    let format = decode_string(&payload[6..22]);
    let labels_raw = decode_string(&payload[22..86]);
    let mut labels: Vec<String> = labels_raw.split(',').map(|s| s.to_string()).collect();

    // Pad with synthetic labels if format has more fields than labels
    let format_len = format.chars().count();
    while labels.len() < format_len {
        labels.push(format!("field_{}", labels.len()));
    }

    Ok(MessageFormat {
        msg_type,
        msg_len,
        name,
        format,
        labels: labels.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_size_all_types() {
        assert_eq!(field_size('b').unwrap(), 1);
        assert_eq!(field_size('B').unwrap(), 1);
        assert_eq!(field_size('M').unwrap(), 1);
        assert_eq!(field_size('h').unwrap(), 2);
        assert_eq!(field_size('H').unwrap(), 2);
        assert_eq!(field_size('c').unwrap(), 2);
        assert_eq!(field_size('C').unwrap(), 2);
        assert_eq!(field_size('i').unwrap(), 4);
        assert_eq!(field_size('I').unwrap(), 4);
        assert_eq!(field_size('e').unwrap(), 4);
        assert_eq!(field_size('E').unwrap(), 4);
        assert_eq!(field_size('f').unwrap(), 4);
        assert_eq!(field_size('L').unwrap(), 4);
        assert_eq!(field_size('n').unwrap(), 4);
        assert_eq!(field_size('q').unwrap(), 8);
        assert_eq!(field_size('Q').unwrap(), 8);
        assert_eq!(field_size('d').unwrap(), 8);
        assert_eq!(field_size('N').unwrap(), 16);
        assert_eq!(field_size('a').unwrap(), 64);
        assert_eq!(field_size('Z').unwrap(), 64);
    }

    #[test]
    fn field_size_invalid() {
        assert!(field_size('x').is_err());
    }

    #[test]
    fn payload_size_known_format() {
        let fmt = MessageFormat {
            msg_type: 0,
            msg_len: 0,
            name: String::new(),
            format: "QccccCCCC".into(),
            labels: Arc::from([]),
        };
        // Q=8, c=2*4, C=2*4 = 8+8+8 = 24
        assert_eq!(fmt.payload_size(), 24);
    }

    #[test]
    fn decode_integer_types() {
        // b: signed i8
        assert_eq!(decode_field('b', &[0xFF]).unwrap(), FieldValue::Int(-1));
        // B: unsigned u8
        assert_eq!(decode_field('B', &[0xFF]).unwrap(), FieldValue::Int(255));
        // h: i16 LE
        assert_eq!(
            decode_field('h', &[0x00, 0x80]).unwrap(),
            FieldValue::Int(-32768)
        );
        // H: u16 LE
        assert_eq!(
            decode_field('H', &[0xFF, 0xFF]).unwrap(),
            FieldValue::Int(65535)
        );
        // i: i32 LE
        assert_eq!(
            decode_field('i', &[0x01, 0x00, 0x00, 0x00]).unwrap(),
            FieldValue::Int(1)
        );
        // I: u32 LE
        assert_eq!(
            decode_field('I', &[0xFF, 0xFF, 0xFF, 0xFF]).unwrap(),
            FieldValue::Int(u32::MAX as i64)
        );
        // q: i64 LE
        let bytes = (-42i64).to_le_bytes();
        assert_eq!(decode_field('q', &bytes).unwrap(), FieldValue::Int(-42));
        // Q: u64 LE
        let bytes = u64::MAX.to_le_bytes();
        assert_eq!(
            decode_field('Q', &bytes).unwrap(),
            FieldValue::Uint(u64::MAX)
        );
        // M: flight mode u8
        assert_eq!(decode_field('M', &[5]).unwrap(), FieldValue::Int(5));
        // L: lat/lon degE7 i32
        let val: i32 = 473_977_000; // ~47.3977 degrees
        assert_eq!(
            decode_field('L', &val.to_le_bytes()).unwrap(),
            FieldValue::Int(val as i64)
        );
    }

    #[test]
    fn decode_float_types() {
        // f: f32
        let v = 1.5f32;
        assert_eq!(
            decode_field('f', &v.to_le_bytes()).unwrap(),
            FieldValue::Float(v as f64)
        );
        // d: f64
        let v = 123.456789f64;
        assert_eq!(
            decode_field('d', &v.to_le_bytes()).unwrap(),
            FieldValue::Float(v)
        );
    }

    #[test]
    fn decode_scaled_types() {
        // c: i16 / 100
        let v: i16 = 4500; // 45.00
        assert_eq!(
            decode_field('c', &v.to_le_bytes()).unwrap(),
            FieldValue::Float(45.0)
        );
        // C: u16 / 100
        let v: u16 = 1234; // 12.34
        assert_eq!(
            decode_field('C', &v.to_le_bytes()).unwrap(),
            FieldValue::Float(12.34)
        );
        // e: i32 / 100
        let v: i32 = -5000; // -50.00
        assert_eq!(
            decode_field('e', &v.to_le_bytes()).unwrap(),
            FieldValue::Float(-50.0)
        );
        // E: u32 / 100
        let v: u32 = 100_000; // 1000.00
        assert_eq!(
            decode_field('E', &v.to_le_bytes()).unwrap(),
            FieldValue::Float(1000.0)
        );
    }

    #[test]
    fn decode_string_types() {
        // n: 4-byte null-padded
        assert_eq!(
            decode_field('n', b"ATT\0").unwrap(),
            FieldValue::String("ATT".into())
        );
        // N: 16-byte null-padded
        let mut buf = [0u8; 16];
        buf[..5].copy_from_slice(b"Hello");
        assert_eq!(
            decode_field('N', &buf).unwrap(),
            FieldValue::String("Hello".into())
        );
        // Z: 64-byte null-padded
        let mut buf = [0u8; 64];
        buf[..11].copy_from_slice(b"Test string");
        assert_eq!(
            decode_field('Z', &buf).unwrap(),
            FieldValue::String("Test string".into())
        );
    }

    #[test]
    fn decode_array_type() {
        let mut buf = [0u8; 64];
        for i in 0..32i16 {
            let bytes = i.to_le_bytes();
            buf[i as usize * 2] = bytes[0];
            buf[i as usize * 2 + 1] = bytes[1];
        }
        let expected: Vec<i16> = (0..32).collect();
        assert_eq!(
            decode_field('a', &buf).unwrap(),
            FieldValue::Array(expected)
        );
    }

    #[test]
    fn decode_fields_values() {
        let fmt = MessageFormat {
            msg_type: 0x81,
            msg_len: 27,
            name: "TEST".into(),
            format: "Qh".into(),
            labels: vec!["TimeUS".into(), "Val".into()].into(),
        };
        let mut payload = Vec::new();
        payload.extend_from_slice(&1000u64.to_le_bytes()); // Q
        payload.extend_from_slice(&(-42i16).to_le_bytes()); // h
        let values = fmt.decode_fields(&payload).unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], FieldValue::Uint(1000));
        assert_eq!(values[1], FieldValue::Int(-42));
    }

    #[test]
    fn parse_fmt_payload_pads_labels() {
        let mut payload = [0u8; 86];
        payload[0] = 0x82;
        payload[1] = 5;
        payload[2..6].copy_from_slice(b"X\0\0\0");
        payload[6..8].copy_from_slice(b"BB");
        payload[22..27].copy_from_slice(b"First");
        let mf = parse_fmt_payload(&payload).unwrap();
        assert_eq!(mf.labels.len(), 2);
        assert_eq!(mf.labels[0], "First");
        assert_eq!(mf.labels[1], "field_1");
    }

    #[test]
    fn parse_fmt_payload_roundtrip() {
        let mut payload = [0u8; 86];
        payload[0] = 0x81; // type
        payload[1] = 27; // length
        payload[2..6].copy_from_slice(b"ATT\0"); // name
        let fmt_str = b"QccccCCCC";
        payload[6..6 + fmt_str.len()].copy_from_slice(fmt_str);
        let labels = b"TimeUS,Roll,Pitch,Yaw,DesRoll,DesPitch,DesYaw,ErrRP,ErrYaw";
        payload[22..22 + labels.len()].copy_from_slice(labels);

        let mf = parse_fmt_payload(&payload).unwrap();
        assert_eq!(mf.msg_type, 0x81);
        assert_eq!(mf.msg_len, 27);
        assert_eq!(mf.name, "ATT");
        assert_eq!(mf.format, "QccccCCCC");
        assert_eq!(mf.labels.len(), 9);
        assert_eq!(mf.labels[0], "TimeUS");
        assert_eq!(mf.labels[8], "ErrYaw");
    }

    #[test]
    fn parse_fmt_payload_too_short() {
        assert!(parse_fmt_payload(&[0u8; 10]).is_err());
    }

    #[test]
    fn string_with_no_null() {
        // Full 4 bytes, no null terminator
        assert_eq!(
            decode_field('n', b"ABCD").unwrap(),
            FieldValue::String("ABCD".into())
        );
    }
}
