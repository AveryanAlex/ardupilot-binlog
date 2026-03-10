use crate::value::FieldValue;

/// A single parsed message from a DataFlash BIN log.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Message type name (e.g. "ATT", "GPS", "BARO")
    pub name: String,

    /// Message type ID (0–255)
    pub msg_type: u8,

    /// Timestamp in microseconds since boot (from the TimeUS field).
    /// None for FMT messages and messages without a Q-typed first field.
    pub timestamp_usec: Option<u64>,

    /// Decoded fields in definition order. Includes TimeUS when present.
    pub fields: Vec<(String, FieldValue)>,
}

impl Entry {
    /// Look up a field by name.
    pub fn get(&self, field: &str) -> Option<&FieldValue> {
        self.fields.iter().find(|(k, _)| k == field).map(|(_, v)| v)
    }

    /// Look up a field and convert to f64 (convenience for charting).
    pub fn get_f64(&self, field: &str) -> Option<f64> {
        self.get(field).and_then(|v| v.as_f64())
    }

    /// Look up a field and convert to string reference.
    pub fn get_str(&self, field: &str) -> Option<&str> {
        self.get(field).and_then(|v| v.as_str())
    }
}
