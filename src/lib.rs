//! Parser for ArduPilot DataFlash BIN log files.
//!
//! ```
//! use ardupilot_binlog::Reader;
//! use std::io::Cursor;
//!
//! let data: Vec<u8> = vec![];
//! let mut reader = Reader::new(Cursor::new(data));
//! for result in &mut reader {
//!     let entry = result.unwrap();
//!     if let Some(roll) = entry.get_f64("Roll") {
//!         println!("roll = {roll}");
//!     }
//! }
//! // Access discovered formats after iteration
//! println!("{} formats discovered", reader.formats().len());
//! ```

mod entry;
mod error;
mod file;
mod format;
mod reader;
mod value;

pub(crate) const HEADER_MAGIC: [u8; 2] = [0xA3, 0x95];
pub(crate) const FMT_TYPE: u8 = 0x80;

pub use entry::Entry;
pub use error::BinlogError;
pub use file::File;
pub use format::MessageFormat;
pub use reader::Reader;
pub use value::FieldValue;
