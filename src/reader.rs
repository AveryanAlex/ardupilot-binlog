use std::collections::HashMap;
use std::io::{BufReader, Read};

use crate::entry::Entry;
use crate::error::BinlogError;
use crate::format::{parse_fmt_payload, MessageFormat};

const HEADER_MAGIC: [u8; 2] = [0xA3, 0x95];
const FMT_TYPE: u8 = 0x80;
const MAX_CONSECUTIVE_ERRORS: u32 = 256;

/// Streaming parser for DataFlash BIN data.
///
/// Reads from any `Read` source, discovers message schemas via FMT
/// messages, and yields parsed entries.
pub struct Reader<R: Read> {
    reader: BufReader<R>,
    formats: HashMap<u8, MessageFormat>,
    consecutive_errors: u32,
}

impl<R: Read> Reader<R> {
    /// Create a new reader wrapping a byte source.
    pub fn new(reader: R) -> Self {
        let mut formats = HashMap::new();
        // Bootstrap: hardcode the FMT definition so we can parse the first FMT message
        formats.insert(
            FMT_TYPE,
            MessageFormat {
                msg_type: FMT_TYPE,
                msg_len: 89,
                name: "FMT".into(),
                format: "BBnNZ".into(),
                labels: vec![
                    "Type".into(),
                    "Length".into(),
                    "Name".into(),
                    "Format".into(),
                    "Labels".into(),
                ],
            },
        );

        Reader {
            reader: BufReader::new(reader),
            formats,
            consecutive_errors: 0,
        }
    }

    /// Read the next entry. Returns Ok(None) at EOF.
    ///
    /// FMT messages are parsed and registered internally, and also
    /// yielded as Entry values so callers can inspect them.
    ///
    /// On parse errors, the reader attempts recovery by scanning for
    /// the next valid header bytes (0xA3, 0x95). After 256 consecutive
    /// failures, returns Ok(None).
    pub fn next_entry(&mut self) -> Result<Option<Entry>, BinlogError> {
        if self.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
            return Ok(None);
        }

        // Read the 3-byte header
        let mut header = [0u8; 3];
        match self.read_exact_or_eof(&mut header) {
            Ok(true) => {}
            Ok(false) => return Ok(None), // EOF
            Err(_) => return Ok(None),
        }

        // Validate magic bytes
        if header[0] != HEADER_MAGIC[0] || header[1] != HEADER_MAGIC[1] {
            self.consecutive_errors += 1;
            if let Some(msg_type) = self.scan_for_header()? {
                return self.parse_message(msg_type);
            }
            return Ok(None);
        }

        let msg_type = header[2];
        self.parse_message(msg_type)
    }

    /// Consume the reader and collect all remaining entries.
    pub fn collect(mut self) -> Result<Vec<Entry>, BinlogError> {
        let mut entries = Vec::new();
        while let Some(entry) = self.next_entry()? {
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Return all message format definitions discovered so far.
    pub fn formats(&self) -> &HashMap<u8, MessageFormat> {
        &self.formats
    }

    /// Parse a message given its type byte (header already consumed).
    fn parse_message(&mut self, msg_type: u8) -> Result<Option<Entry>, BinlogError> {
        // Look up format
        let format = match self.formats.get(&msg_type) {
            Some(f) => f.clone(),
            None => {
                self.consecutive_errors += 1;
                return self.recover_and_retry();
            }
        };

        // Read payload
        let payload_len = format.msg_len as usize - 3;
        let mut payload = vec![0u8; payload_len];
        match self.read_exact_or_eof(&mut payload) {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                self.consecutive_errors += 1;
                return self.recover_and_retry();
            }
        }

        // Handle FMT messages
        if msg_type == FMT_TYPE {
            match parse_fmt_payload(&payload) {
                Ok(new_fmt) => {
                    let fields = match format.decode_fields(&payload) {
                        Ok(f) => f,
                        Err(_) => {
                            self.consecutive_errors += 1;
                            return self.recover_and_retry();
                        }
                    };
                    let entry = Entry {
                        name: "FMT".into(),
                        msg_type: FMT_TYPE,
                        timestamp_usec: None,
                        fields,
                    };
                    self.formats.insert(new_fmt.msg_type, new_fmt);
                    self.consecutive_errors = 0;
                    return Ok(Some(entry));
                }
                Err(_) => {
                    self.consecutive_errors += 1;
                    return self.recover_and_retry();
                }
            }
        }

        // Decode fields
        let fields = match format.decode_fields(&payload) {
            Ok(f) => f,
            Err(_) => {
                self.consecutive_errors += 1;
                return self.recover_and_retry();
            }
        };

        // Extract timestamp from first field:
        // - Modern logs: format char 'Q', label "TimeUS" (microseconds)
        // - Older logs: format char 'I', label "TimeMS" (milliseconds → convert to µs)
        let timestamp_usec = match format.format.chars().next() {
            Some('Q') => fields.first().and_then(|(_, v)| v.as_u64()),
            Some('I') => {
                let is_time_field = format
                    .labels
                    .first()
                    .map(|l| l == "TimeMS" || l == "TimeUS")
                    .unwrap_or(false);
                if is_time_field {
                    fields
                        .first()
                        .and_then(|(_, v)| v.as_i64())
                        .map(|ms| ms as u64 * 1000)
                } else {
                    None
                }
            }
            _ => None,
        };

        let entry = Entry {
            name: format.name.clone(),
            msg_type,
            timestamp_usec,
            fields,
        };

        self.consecutive_errors = 0;
        Ok(Some(entry))
    }

    /// Scan forward byte-by-byte looking for the magic header, then retry parsing.
    fn recover_and_retry(&mut self) -> Result<Option<Entry>, BinlogError> {
        if self.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
            return Ok(None);
        }
        match self.scan_for_header()? {
            Some(msg_type) => self.parse_message(msg_type),
            None => Ok(None),
        }
    }

    /// Scan byte-by-byte for the next 0xA3 0x95 sequence.
    /// Returns the msg_type byte that follows the magic, or None at EOF.
    fn scan_for_header(&mut self) -> Result<Option<u8>, BinlogError> {
        let mut prev = 0u8;
        loop {
            let mut byte = [0u8; 1];
            match self.reader.read(&mut byte) {
                Ok(0) => return Ok(None), // EOF
                Ok(_) => {
                    if prev == HEADER_MAGIC[0] && byte[0] == HEADER_MAGIC[1] {
                        // Found magic, read msg_type
                        let mut msg_type = [0u8; 1];
                        match self.reader.read(&mut msg_type) {
                            Ok(0) => return Ok(None),
                            Ok(_) => return Ok(Some(msg_type[0])),
                            Err(_) => return Ok(None),
                        }
                    }
                    prev = byte[0];
                }
                Err(_) => return Ok(None),
            }
        }
    }

    /// Read exactly `buf.len()` bytes. Returns Ok(true) on success, Ok(false) on EOF
    /// (zero bytes available), Err on partial read / IO error.
    fn read_exact_or_eof(&mut self, buf: &mut [u8]) -> Result<bool, BinlogError> {
        let mut total = 0;
        while total < buf.len() {
            match self.reader.read(&mut buf[total..]) {
                Ok(0) => {
                    if total == 0 {
                        return Ok(false); // Clean EOF
                    }
                    return Err(BinlogError::UnexpectedEof);
                }
                Ok(n) => total += n,
                Err(e) => return Err(BinlogError::Io(e)),
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::FieldValue;

    /// Build a valid FMT message (89 bytes) that defines the FMT type itself.
    fn build_fmt_bootstrap() -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&HEADER_MAGIC);
        msg.push(FMT_TYPE);
        // 86-byte payload
        let mut payload = [0u8; 86];
        payload[0] = FMT_TYPE; // type
        payload[1] = 89; // length
        payload[2..6].copy_from_slice(b"FMT\0"); // name
        payload[6..11].copy_from_slice(b"BBnNZ"); // format
        let labels = b"Type,Length,Name,Format,Labels";
        payload[22..22 + labels.len()].copy_from_slice(labels);
        msg.extend_from_slice(&payload);
        msg
    }

    /// Build a FMT message that defines a custom message type.
    fn build_fmt_for_type(
        msg_type: u8,
        msg_len: u8,
        name: &[u8; 4],
        format: &str,
        labels: &str,
    ) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&HEADER_MAGIC);
        msg.push(FMT_TYPE);
        let mut payload = [0u8; 86];
        payload[0] = msg_type;
        payload[1] = msg_len;
        payload[2..6].copy_from_slice(name);
        let fmt_bytes = format.as_bytes();
        payload[6..6 + fmt_bytes.len()].copy_from_slice(fmt_bytes);
        let lbl_bytes = labels.as_bytes();
        payload[22..22 + lbl_bytes.len()].copy_from_slice(lbl_bytes);
        msg.extend_from_slice(&payload);
        msg
    }

    /// Build a data message with a given type and raw payload.
    fn build_data_message(msg_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&HEADER_MAGIC);
        msg.push(msg_type);
        msg.extend_from_slice(payload);
        msg
    }

    #[test]
    fn parse_empty_input() {
        let reader = Reader::new(std::io::Cursor::new(Vec::new()));
        let entries = reader.collect().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_fmt_bootstrap_only() {
        let data = build_fmt_bootstrap();
        let reader = Reader::new(std::io::Cursor::new(data));
        let entries = reader.collect().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "FMT");
        assert_eq!(entries[0].msg_type, FMT_TYPE);
        assert!(entries[0].timestamp_usec.is_none());
    }

    #[test]
    fn parse_data_message() {
        let mut data = Vec::new();
        // Bootstrap FMT
        data.extend(build_fmt_bootstrap());
        // Define ATT: type 0x81, format "Qhh", labels "TimeUS,Roll,Pitch"
        // payload size: 8 + 2 + 2 = 12, total = 15
        data.extend(build_fmt_for_type(
            0x81,
            15,
            b"ATT\0",
            "Qhh",
            "TimeUS,Roll,Pitch",
        ));
        // ATT data message
        let mut payload = Vec::new();
        payload.extend_from_slice(&1_000_000u64.to_le_bytes()); // TimeUS
        payload.extend_from_slice(&4500i16.to_le_bytes()); // Roll
        payload.extend_from_slice(&(-200i16).to_le_bytes()); // Pitch
        data.extend(build_data_message(0x81, &payload));

        let reader = Reader::new(std::io::Cursor::new(data));
        let entries = reader.collect().unwrap();
        assert_eq!(entries.len(), 3); // FMT(FMT), FMT(ATT), ATT

        let att = &entries[2];
        assert_eq!(att.name, "ATT");
        assert_eq!(att.msg_type, 0x81);
        assert_eq!(att.timestamp_usec, Some(1_000_000));
        assert_eq!(att.get("Roll"), Some(&FieldValue::Int(4500)));
        assert_eq!(att.get("Pitch"), Some(&FieldValue::Int(-200)));
    }

    #[test]
    fn error_recovery_with_garbage() {
        let mut data = Vec::new();
        data.extend(build_fmt_bootstrap());
        // Define a simple type
        data.extend(build_fmt_for_type(
            0x81, 11, // 3 + 8
            b"TST\0", "Q", "TimeUS",
        ));
        // First valid data message
        data.extend(build_data_message(0x81, &100u64.to_le_bytes()));
        // Garbage bytes
        data.extend_from_slice(&[0xFF; 50]);
        // Second valid data message
        data.extend(build_data_message(0x81, &200u64.to_le_bytes()));

        let reader = Reader::new(std::io::Cursor::new(data));
        let entries = reader.collect().unwrap();

        // Should have: FMT(FMT), FMT(TST), TST(100), TST(200)
        let tst_entries: Vec<_> = entries.iter().filter(|e| e.name == "TST").collect();
        assert_eq!(tst_entries.len(), 2);
        assert_eq!(tst_entries[0].timestamp_usec, Some(100));
        assert_eq!(tst_entries[1].timestamp_usec, Some(200));
    }

    #[test]
    fn truncated_final_message() {
        let mut data = Vec::new();
        data.extend(build_fmt_bootstrap());
        data.extend(build_fmt_for_type(0x81, 11, b"TST\0", "Q", "TimeUS"));
        // Valid message
        data.extend(build_data_message(0x81, &100u64.to_le_bytes()));
        // Truncated message: header + partial payload
        data.extend_from_slice(&HEADER_MAGIC);
        data.push(0x81);
        data.extend_from_slice(&[0; 3]); // only 3 of 8 payload bytes

        let reader = Reader::new(std::io::Cursor::new(data));
        let entries = reader.collect().unwrap();

        let tst_entries: Vec<_> = entries.iter().filter(|e| e.name == "TST").collect();
        assert_eq!(tst_entries.len(), 1);
        assert_eq!(tst_entries[0].timestamp_usec, Some(100));
    }

    #[test]
    fn unknown_type_recovery() {
        let mut data = Vec::new();
        data.extend(build_fmt_bootstrap());
        data.extend(build_fmt_for_type(0x81, 11, b"TST\0", "Q", "TimeUS"));
        // Message with unknown type 0x99
        data.extend_from_slice(&HEADER_MAGIC);
        data.push(0x99);
        data.extend_from_slice(&[0; 20]); // some bytes
                                          // Valid message after
        data.extend(build_data_message(0x81, &300u64.to_le_bytes()));

        let reader = Reader::new(std::io::Cursor::new(data));
        let entries = reader.collect().unwrap();

        let tst_entries: Vec<_> = entries.iter().filter(|e| e.name == "TST").collect();
        assert_eq!(tst_entries.len(), 1);
        assert_eq!(tst_entries[0].timestamp_usec, Some(300));
    }

    #[test]
    fn formats_accessible() {
        let data = build_fmt_bootstrap();
        let mut reader = Reader::new(std::io::Cursor::new(data));
        let _ = reader.next_entry(); // parse the FMT
        assert!(reader.formats().contains_key(&FMT_TYPE));
        assert_eq!(reader.formats().get(&FMT_TYPE).unwrap().name, "FMT");
    }
}
