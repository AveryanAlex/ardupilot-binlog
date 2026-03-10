#![no_main]
use libfuzzer_sys::fuzz_target;

use ardupilot_binlog::Reader;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let reader = Reader::new(Cursor::new(data));
    // Consume all entries, ignoring errors — should never panic
    for _ in reader {}
});
