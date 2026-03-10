/// A decoded field value from a DataFlash log entry.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    /// Integer values (b, B, h, H, i, I, q, M, L)
    Int(i64),
    /// Unsigned 64-bit integer (Q) — separate because u64 can exceed i64 range
    Uint(u64),
    /// Floating-point values (f, d) and pre-scaled values (c, C, e, E already divided by 100)
    Float(f64),
    /// String values (n, N, Z) — null bytes trimmed
    String(String),
    /// Array of i16 values (a)
    Array(Vec<i16>),
}

impl FieldValue {
    /// Convert to f64 for numeric use. Returns None for String and Array variants.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            FieldValue::Int(v) => Some(*v as f64),
            FieldValue::Uint(v) => Some(*v as f64),
            FieldValue::Float(v) => Some(*v),
            FieldValue::String(_) | FieldValue::Array(_) => None,
        }
    }

    /// Convert to i64. Returns None for non-integer variants.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            FieldValue::Int(v) => Some(*v),
            _ => None,
        }
    }

    /// Convert to u64. Returns None for non-Uint variants.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            FieldValue::Uint(v) => Some(*v),
            _ => None,
        }
    }

    /// Get string reference. Returns None for non-string variants.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            FieldValue::String(s) => Some(s),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_f64_int() {
        assert_eq!(FieldValue::Int(42).as_f64(), Some(42.0));
        assert_eq!(FieldValue::Int(-100).as_f64(), Some(-100.0));
    }

    #[test]
    fn as_f64_uint() {
        assert_eq!(FieldValue::Uint(u64::MAX).as_f64(), Some(u64::MAX as f64));
    }

    #[test]
    fn as_f64_float() {
        assert_eq!(FieldValue::Float(1.23).as_f64(), Some(1.23));
    }

    #[test]
    fn as_f64_string_returns_none() {
        assert_eq!(FieldValue::String("hello".into()).as_f64(), None);
    }

    #[test]
    fn as_f64_array_returns_none() {
        assert_eq!(FieldValue::Array(vec![1, 2, 3]).as_f64(), None);
    }

    #[test]
    fn as_i64() {
        assert_eq!(FieldValue::Int(-5).as_i64(), Some(-5));
        assert_eq!(FieldValue::Uint(5).as_i64(), None);
        assert_eq!(FieldValue::Float(5.0).as_i64(), None);
    }

    #[test]
    fn as_u64() {
        assert_eq!(FieldValue::Uint(123).as_u64(), Some(123));
        assert_eq!(FieldValue::Int(123).as_u64(), None);
    }

    #[test]
    fn as_str() {
        assert_eq!(FieldValue::String("test".into()).as_str(), Some("test"));
        assert_eq!(FieldValue::Int(0).as_str(), None);
    }
}
