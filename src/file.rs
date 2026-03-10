use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::BinlogError;
use crate::format::MessageFormat;
use crate::reader::Reader;

const HEADER_MAGIC: [u8; 2] = [0xA3, 0x95];
const TAIL_SCAN_SIZE: u64 = 65536;

/// High-level wrapper for reading a DataFlash BIN file from disk.
pub struct File {
    path: PathBuf,
}

impl File {
    /// Open a BIN file. Validates the path exists.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BinlogError> {
        let path = path.as_ref().to_path_buf();
        // Validate the path exists by checking metadata
        fs::metadata(&path)?;
        Ok(File { path })
    }

    /// Return a fresh reader over the file's entries.
    pub fn entries(&self) -> Result<Reader<BufReader<fs::File>>, BinlogError> {
        let file = fs::File::open(&self.path)?;
        Ok(Reader::new(BufReader::new(file)))
    }

    /// Scan the file and return the time range (first_usec, last_usec).
    ///
    /// Optimized: reads the head to find FMTs and the first timestamped
    /// message, then scans the tail for the last timestamp.
    /// Returns None for empty files or files with no timestamped entries.
    ///
    /// Timestamps are boot-relative, not Unix epoch.
    pub fn time_range(&self) -> Result<Option<(u64, u64)>, BinlogError> {
        let metadata = fs::metadata(&self.path)?;
        let file_size = metadata.len();
        if file_size == 0 {
            return Ok(None);
        }

        // Head scan: collect FMTs and find the first timestamp
        let mut reader = self.entries()?;
        let mut first_ts: Option<u64> = None;

        // Parse entries to collect all FMTs and find first timestamp
        while let Some(entry) = reader.next_entry()? {
            if let Some(ts) = entry.timestamp_usec {
                first_ts = Some(ts);
                break;
            }
        }

        let first_ts = match first_ts {
            Some(ts) => ts,
            None => return Ok(None),
        };

        // Get the format table from the head scan
        let formats = reader.formats().clone();

        // Tail scan: seek near end and scan for timestamps
        let mut file = fs::File::open(&self.path)?;
        let seek_pos = file_size.saturating_sub(TAIL_SCAN_SIZE);
        file.seek(SeekFrom::Start(seek_pos))?;

        let mut tail_buf = Vec::new();
        file.read_to_end(&mut tail_buf)?;

        let last_ts = scan_tail_for_last_timestamp(&tail_buf, &formats);

        match last_ts {
            Some(ts) => Ok(Some((first_ts, ts.max(first_ts)))),
            None => Ok(Some((first_ts, first_ts))),
        }
    }
}

/// Scan a byte buffer for valid message headers and extract the maximum timestamp.
/// Uses the format table to validate message types and sizes.
fn scan_tail_for_last_timestamp(buf: &[u8], formats: &HashMap<u8, MessageFormat>) -> Option<u64> {
    let mut max_ts: Option<u64> = None;
    let mut pos = 0;

    while pos + 3 <= buf.len() {
        // Look for magic header
        if buf[pos] == HEADER_MAGIC[0] && pos + 1 < buf.len() && buf[pos + 1] == HEADER_MAGIC[1] {
            let msg_type = buf[pos + 2];
            if let Some(fmt) = formats.get(&msg_type) {
                let msg_len = fmt.msg_len as usize;
                if pos + msg_len <= buf.len() {
                    // Extract timestamp from first field
                    let first_char = fmt.format.chars().next();
                    let is_time_label = fmt
                        .labels
                        .first()
                        .map(|l| l == "TimeMS" || l == "TimeUS")
                        .unwrap_or(false);
                    let ts_start = pos + 3;
                    if first_char == Some('Q') && msg_len >= 3 + 8 {
                        let ts_bytes: [u8; 8] = buf[ts_start..ts_start + 8].try_into().unwrap();
                        let ts = u64::from_le_bytes(ts_bytes);
                        max_ts = Some(max_ts.map_or(ts, |prev: u64| prev.max(ts)));
                    } else if first_char == Some('I') && is_time_label && msg_len >= 3 + 4 {
                        let ts_bytes: [u8; 4] = buf[ts_start..ts_start + 4].try_into().unwrap();
                        let ms = u32::from_le_bytes(ts_bytes) as u64;
                        let ts = ms * 1000;
                        max_ts = Some(max_ts.map_or(ts, |prev: u64| prev.max(ts)));
                    }
                    pos += msg_len;
                    continue;
                }
            }
        }
        pos += 1;
    }

    max_ts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn open_nonexistent_file() {
        assert!(File::open("/tmp/definitely_does_not_exist_12345.bin").is_err());
    }

    #[test]
    fn open_and_entries_empty_file() {
        let dir = std::env::temp_dir().join("binlog_test_empty");
        fs::create_dir_all(&dir).ok();
        let path = dir.join("empty.bin");
        fs::write(&path, []).unwrap();

        let file = File::open(&path).unwrap();
        let entries = file.entries().unwrap().collect().unwrap();
        assert!(entries.is_empty());

        assert_eq!(file.time_range().unwrap(), None);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn time_range_with_data() {
        let dir = std::env::temp_dir().join("binlog_test_time_range");
        fs::create_dir_all(&dir).ok();
        let path = dir.join("time_range.bin");

        let mut data = Vec::new();

        // FMT bootstrap
        data.extend_from_slice(&HEADER_MAGIC);
        data.push(0x80);
        let mut payload = [0u8; 86];
        payload[0] = 0x80;
        payload[1] = 89;
        payload[2..6].copy_from_slice(b"FMT\0");
        payload[6..11].copy_from_slice(b"BBnNZ");
        let labels = b"Type,Length,Name,Format,Labels";
        payload[22..22 + labels.len()].copy_from_slice(labels);
        data.extend_from_slice(&payload);

        // Define TST type: format "Q", total len = 11
        data.extend_from_slice(&HEADER_MAGIC);
        data.push(0x80);
        let mut payload = [0u8; 86];
        payload[0] = 0x81;
        payload[1] = 11;
        payload[2..6].copy_from_slice(b"TST\0");
        payload[6..7].copy_from_slice(b"Q");
        payload[22..28].copy_from_slice(b"TimeUS");
        data.extend_from_slice(&payload);

        // TST message with timestamp 1000
        data.extend_from_slice(&HEADER_MAGIC);
        data.push(0x81);
        data.extend_from_slice(&1000u64.to_le_bytes());

        // TST message with timestamp 5000
        data.extend_from_slice(&HEADER_MAGIC);
        data.push(0x81);
        data.extend_from_slice(&5000u64.to_le_bytes());

        let mut f = fs::File::create(&path).unwrap();
        f.write_all(&data).unwrap();
        drop(f);

        let file = File::open(&path).unwrap();
        let range = file.time_range().unwrap();
        assert_eq!(range, Some((1000, 5000)));

        fs::remove_file(&path).ok();
    }

    #[test]
    fn time_range_fmt_only() {
        let dir = std::env::temp_dir().join("binlog_test_fmt_only");
        fs::create_dir_all(&dir).ok();
        let path = dir.join("fmt_only.bin");

        let mut data = Vec::new();
        // FMT bootstrap only — no timestamped messages
        data.extend_from_slice(&HEADER_MAGIC);
        data.push(0x80);
        let mut payload = [0u8; 86];
        payload[0] = 0x80;
        payload[1] = 89;
        payload[2..6].copy_from_slice(b"FMT\0");
        payload[6..11].copy_from_slice(b"BBnNZ");
        let labels = b"Type,Length,Name,Format,Labels";
        payload[22..22 + labels.len()].copy_from_slice(labels);
        data.extend_from_slice(&payload);

        fs::write(&path, &data).unwrap();

        let file = File::open(&path).unwrap();
        assert_eq!(file.time_range().unwrap(), None);

        fs::remove_file(&path).ok();
    }
}
