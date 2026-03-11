mod common;

use ardupilot_binlog::{Entry, FieldValue, File, Reader};
use std::io::Cursor;

#[test]
fn parse_real_fixture() {
    let file = File::open("tests/fixtures/short-flight.bin").unwrap();
    let entries: Vec<_> = file
        .entries()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // Should have many entries
    assert!(
        entries.len() > 100,
        "expected many entries, got {}",
        entries.len()
    );

    // First entries should be FMTs
    assert_eq!(entries[0].name, "FMT");
    assert_eq!(entries[0].msg_type, 0x80);

    // Count FMT entries — they should all be at the start
    let fmt_count = entries.iter().take_while(|e| e.name == "FMT").count();
    assert!(
        fmt_count > 5,
        "expected multiple FMT entries, got {}",
        fmt_count
    );
}

#[test]
fn fixture_format_discovery() {
    let file = File::open("tests/fixtures/short-flight.bin").unwrap();
    let mut reader = file.entries().unwrap();

    // Parse all entries
    for result in reader.by_ref() {
        result.unwrap();
    }

    let formats = reader.formats();
    // Should have discovered multiple message types
    assert!(
        formats.len() > 5,
        "expected many formats, got {}",
        formats.len()
    );

    // FMT should always be present
    assert!(formats.contains_key(&0x80));

    // Print discovered format names for debugging
    let names: Vec<&str> = formats.values().map(|f| f.name.as_str()).collect();
    assert!(
        names.contains(&"FMT"),
        "missing FMT format, got: {:?}",
        names
    );
}

#[test]
fn fixture_has_plausible_data() {
    let file = File::open("tests/fixtures/short-flight.bin").unwrap();
    let entries: Vec<_> = file
        .entries()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // Check that non-FMT entries have timestamps
    let data_entries: Vec<&Entry> = entries.iter().filter(|e| e.name != "FMT").collect();
    assert!(!data_entries.is_empty());

    let with_timestamp: Vec<&&Entry> = data_entries
        .iter()
        .filter(|e| e.timestamp_usec.is_some())
        .collect();
    assert!(
        !with_timestamp.is_empty(),
        "no entries with timestamps found"
    );
}

#[test]
fn fixture_parm_entries_have_strings() {
    let file = File::open("tests/fixtures/short-flight.bin").unwrap();
    let entries: Vec<_> = file
        .entries()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let parm_entries: Vec<&Entry> = entries.iter().filter(|e| e.name == "PARM").collect();
    if !parm_entries.is_empty() {
        // PARM entries should have a Name field that is a readable string
        let name_val = parm_entries[0].get_str("Name");
        assert!(name_val.is_some(), "PARM entry missing Name string field");
        assert!(!name_val.unwrap().is_empty(), "PARM Name field is empty");
    }
}

#[test]
fn fixture_time_range() {
    let file = File::open("tests/fixtures/short-flight.bin").unwrap();
    let range = file.time_range().unwrap();
    assert!(
        range.is_some(),
        "time_range should return Some for non-empty file"
    );

    let (first, last) = range.unwrap();
    assert!(
        last >= first,
        "last timestamp should be >= first: {} vs {}",
        first,
        last
    );
    assert!(
        last > first,
        "expected some time span between first and last"
    );
}

// ---- Synthetic data tests ----

#[test]
fn synthetic_error_recovery() {
    let mut data = Vec::new();
    data.extend(common::build_fmt_bootstrap());
    // Define TST: type 0x81, format "Q", total len = 11
    data.extend(common::build_fmt_for_type(
        0x81, 11, b"TST\0", "Q", "TimeUS",
    ));
    // First valid message
    data.extend(common::build_data_message(0x81, &100u64.to_le_bytes()));
    // 50 bytes of garbage
    data.extend_from_slice(&[0xDE; 50]);
    // Second valid message
    data.extend(common::build_data_message(0x81, &200u64.to_le_bytes()));

    let reader = Reader::new(Cursor::new(data));
    let entries: Vec<_> = reader.collect::<Result<Vec<_>, _>>().unwrap();

    let tst: Vec<&Entry> = entries.iter().filter(|e| e.name == "TST").collect();
    assert_eq!(tst.len(), 2, "expected 2 TST entries after recovery");
    assert_eq!(tst[0].timestamp_usec, Some(100));
    assert_eq!(tst[1].timestamp_usec, Some(200));
}

#[test]
fn synthetic_empty() {
    let reader = Reader::new(Cursor::new(Vec::<u8>::new()));
    let entries: Vec<_> = reader.collect::<Result<Vec<_>, _>>().unwrap();
    assert!(entries.is_empty());
}

#[test]
fn synthetic_fmt_only() {
    let data = common::build_fmt_bootstrap();
    let reader = Reader::new(Cursor::new(data));
    let entries: Vec<_> = reader.collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "FMT");
    assert!(entries[0].timestamp_usec.is_none());
}

#[test]
fn synthetic_truncated_final_message() {
    let mut data = Vec::new();
    data.extend(common::build_fmt_bootstrap());
    data.extend(common::build_fmt_for_type(
        0x81, 11, b"TST\0", "Q", "TimeUS",
    ));
    // Valid message
    data.extend(common::build_data_message(0x81, &100u64.to_le_bytes()));
    // Truncated message — header only, no complete payload
    data.extend_from_slice(&[0xA3, 0x95]);
    data.push(0x81);
    data.extend_from_slice(&[0; 4]); // only 4 of 8 bytes

    let reader = Reader::new(Cursor::new(data));
    let entries: Vec<_> = reader.collect::<Result<Vec<_>, _>>().unwrap();

    let tst: Vec<&Entry> = entries.iter().filter(|e| e.name == "TST").collect();
    assert_eq!(
        tst.len(),
        1,
        "truncated message should not produce an entry"
    );
    assert_eq!(tst[0].timestamp_usec, Some(100));
}

#[test]
fn synthetic_scaled_fields() {
    let mut data = Vec::new();
    data.extend(common::build_fmt_bootstrap());
    // Define type with scaled fields: "QcCeE"
    // Q=8, c=2, C=2, e=4, E=4 = 20 payload, total=23
    data.extend(common::build_fmt_for_type(
        0x82,
        23,
        b"SCL\0",
        "QcCeE",
        "TimeUS,A,B,C,D",
    ));

    let mut payload = Vec::new();
    payload.extend_from_slice(&1000u64.to_le_bytes()); // Q: TimeUS
    payload.extend_from_slice(&4500i16.to_le_bytes()); // c: 4500 / 100 = 45.0
    payload.extend_from_slice(&1234u16.to_le_bytes()); // C: 1234 / 100 = 12.34
    payload.extend_from_slice(&(-5000i32).to_le_bytes()); // e: -5000 / 100 = -50.0
    payload.extend_from_slice(&100_000u32.to_le_bytes()); // E: 100000 / 100 = 1000.0
    data.extend(common::build_data_message(0x82, &payload));

    let reader = Reader::new(Cursor::new(data));
    let entries: Vec<_> = reader.collect::<Result<Vec<_>, _>>().unwrap();

    let scl: Vec<&Entry> = entries.iter().filter(|e| e.name == "SCL").collect();
    assert_eq!(scl.len(), 1);

    let e = scl[0];
    assert_eq!(e.get_f64("A"), Some(45.0));
    assert_eq!(e.get_f64("B"), Some(12.34));
    assert_eq!(e.get_f64("C"), Some(-50.0));
    assert_eq!(e.get_f64("D"), Some(1000.0));
}

#[test]
fn synthetic_string_fields() {
    let mut data = Vec::new();
    data.extend(common::build_fmt_bootstrap());
    // MSG type: "QZ" format, total = 3 + 8 + 64 = 75
    data.extend(common::build_fmt_for_type(
        0x83,
        75,
        b"MSG\0",
        "QZ",
        "TimeUS,Message",
    ));

    let mut payload = Vec::new();
    payload.extend_from_slice(&500u64.to_le_bytes());
    let mut msg_bytes = [0u8; 64];
    msg_bytes[..11].copy_from_slice(b"Hello World");
    payload.extend_from_slice(&msg_bytes);
    data.extend(common::build_data_message(0x83, &payload));

    let reader = Reader::new(Cursor::new(data));
    let entries: Vec<_> = reader.collect::<Result<Vec<_>, _>>().unwrap();

    let msg: Vec<&Entry> = entries.iter().filter(|e| e.name == "MSG").collect();
    assert_eq!(msg.len(), 1);
    assert_eq!(msg[0].get_str("Message"), Some("Hello World"));
    assert_eq!(msg[0].timestamp_usec, Some(500));
}

#[test]
fn synthetic_multiple_message_types() {
    let mut data = Vec::new();
    data.extend(common::build_fmt_bootstrap());

    // Define two types
    data.extend(common::build_fmt_for_type(
        0x81, 11, b"TST\0", "Q", "TimeUS",
    ));
    data.extend(common::build_fmt_for_type(
        0x82,
        15,
        b"DAT\0",
        "Qhh",
        "TimeUS,X,Y",
    ));

    // Interleave messages
    data.extend(common::build_data_message(0x81, &100u64.to_le_bytes()));

    let mut dat_payload = Vec::new();
    dat_payload.extend_from_slice(&200u64.to_le_bytes());
    dat_payload.extend_from_slice(&42i16.to_le_bytes());
    dat_payload.extend_from_slice(&(-7i16).to_le_bytes());
    data.extend(common::build_data_message(0x82, &dat_payload));

    data.extend(common::build_data_message(0x81, &300u64.to_le_bytes()));

    let reader = Reader::new(Cursor::new(data));
    let entries: Vec<_> = reader.collect::<Result<Vec<_>, _>>().unwrap();

    let tst: Vec<&Entry> = entries.iter().filter(|e| e.name == "TST").collect();
    let dat: Vec<&Entry> = entries.iter().filter(|e| e.name == "DAT").collect();

    assert_eq!(tst.len(), 2);
    assert_eq!(dat.len(), 1);
    assert_eq!(dat[0].get("X"), Some(&FieldValue::Int(42)));
    assert_eq!(dat[0].get("Y"), Some(&FieldValue::Int(-7)));
}

#[test]
fn golden_snapshot() {
    let file = File::open("tests/fixtures/short-flight.bin").unwrap();
    let entries: Vec<_> = file
        .entries()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // Exact total entry count
    assert_eq!(entries.len(), 6290, "total entry count");

    // Exact total field count across all entries
    let total_fields: usize = entries.iter().map(|e| e.len()).sum();
    assert_eq!(total_fields, 54277, "total field count");

    // Exact first and last timestamps
    let timestamps: Vec<u64> = entries.iter().filter_map(|e| e.timestamp_usec).collect();
    assert_eq!(timestamps.first(), Some(&11459000), "first timestamp");
    assert_eq!(timestamps.last(), Some(&26729000), "last timestamp");

    // Per-type counts: count occurrences of each message type
    let count = |name: &str| -> usize { entries.iter().filter(|e| e.name == name).count() };

    // FMT and PARM (required)
    assert_eq!(count("FMT"), 72, "FMT count");
    assert_eq!(count("PARM"), 491, "PARM count");

    // At least 3 additional concrete message types
    assert_eq!(count("ATT"), 150, "ATT count");
    assert_eq!(count("GPS"), 73, "GPS count");
    assert_eq!(count("IMU"), 749, "IMU count");
    assert_eq!(count("BARO"), 150, "BARO count");
    assert_eq!(count("MAG"), 150, "MAG count");
}
