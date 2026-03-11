use std::sync::Arc;

use crate::value::FieldValue;

/// A single parsed message from a DataFlash BIN log.
///
/// ```
/// use ardupilot_binlog::Reader;
/// use std::io::Cursor;
///
/// let reader = Reader::new(Cursor::new(Vec::new()));
/// for result in reader {
///     let entry = result.unwrap();
///     println!("{} ({} fields)", entry.name, entry.len());
///     for (label, value) in entry.fields() {
///         println!("  {label} = {value}");
///     }
/// }
/// ```
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(into = "EntryRepr", from = "EntryRepr"))]
pub struct Entry {
    /// Message type name (e.g. "ATT", "GPS", "BARO")
    pub name: String,

    /// Message type ID (0–255)
    pub msg_type: u8,

    /// Timestamp in microseconds since boot (from the first Q-typed field).
    /// None for FMT messages and messages without a Q-typed first field.
    pub timestamp_usec: Option<u64>,

    pub(crate) labels: Arc<[String]>,
    pub(crate) values: Vec<FieldValue>,
}

impl Entry {
    /// Look up a field by name.
    pub fn get(&self, field: &str) -> Option<&FieldValue> {
        let idx = self.labels.iter().position(|l| l == field)?;
        self.values.get(idx)
    }

    /// Look up a field and convert to f64 (convenience for charting).
    pub fn get_f64(&self, field: &str) -> Option<f64> {
        self.get(field).and_then(|v| v.as_f64())
    }

    /// Look up a field and convert to i64.
    pub fn get_i64(&self, field: &str) -> Option<i64> {
        self.get(field).and_then(|v| v.as_i64())
    }

    /// Look up a field and convert to u64.
    pub fn get_u64(&self, field: &str) -> Option<u64> {
        self.get(field).and_then(|v| v.as_u64())
    }

    /// Look up a field and convert to string reference.
    pub fn get_str(&self, field: &str) -> Option<&str> {
        self.get(field).and_then(|v| v.as_str())
    }

    /// Iterate over (label, value) pairs in definition order.
    pub fn fields(&self) -> impl Iterator<Item = (&str, &FieldValue)> {
        self.labels
            .iter()
            .map(|l| l.as_str())
            .zip(self.values.iter())
    }

    /// Return field labels in definition order.
    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// Return field values in definition order.
    #[must_use]
    pub fn values(&self) -> &[FieldValue] {
        &self.values
    }

    /// Return the number of fields.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Return true if the entry has no fields.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

// Serde helper: serialize Entry as a flat struct with fields as Vec<(String, FieldValue)>
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
struct EntryRepr {
    name: String,
    msg_type: u8,
    timestamp_usec: Option<u64>,
    fields: Vec<(String, FieldValue)>,
}

#[cfg(feature = "serde")]
impl From<Entry> for EntryRepr {
    fn from(e: Entry) -> Self {
        let fields = e.labels.iter().cloned().zip(e.values).collect();
        EntryRepr {
            name: e.name,
            msg_type: e.msg_type,
            timestamp_usec: e.timestamp_usec,
            fields,
        }
    }
}

#[cfg(feature = "serde")]
impl From<EntryRepr> for Entry {
    fn from(r: EntryRepr) -> Self {
        let (labels, values): (Vec<String>, Vec<FieldValue>) = r.fields.into_iter().unzip();
        Entry {
            name: r.name,
            msg_type: r.msg_type,
            timestamp_usec: r.timestamp_usec,
            labels: labels.into(),
            values,
        }
    }
}
