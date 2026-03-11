# ardupilot-binlog

[![Crates.io](https://img.shields.io/crates/v/ardupilot-binlog)](https://crates.io/crates/ardupilot-binlog)
[![docs.rs](https://img.shields.io/docsrs/ardupilot-binlog)](https://docs.rs/ardupilot-binlog)
[![CI](https://github.com/AveryanAlex/ardupilot-binlog/actions/workflows/ci.yml/badge.svg)](https://github.com/AveryanAlex/ardupilot-binlog/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/AveryanAlex/ardupilot-binlog/branch/main/graph/badge.svg)](https://codecov.io/gh/AveryanAlex/ardupilot-binlog)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/ardupilot-binlog)](https://github.com/AveryanAlex/ardupilot-binlog/blob/main/LICENSE-MIT)

Parser for ArduPilot DataFlash BIN log files.

Reads `.bin` / `.BIN` files produced by ArduPilot's onboard DataFlash logger. The format is self-describing — message schemas are discovered from FMT messages within each file, so this crate works with any ArduPilot version without hardcoded message definitions.

## Usage

```toml
[dependencies]
ardupilot-binlog = "0.2"
```

### Parse all entries

```rust
use ardupilot_binlog::File;

let file = File::open("flight.bin")?;
let mut reader = file.entries()?;

for result in &mut reader {
    let entry = result?;
    if entry.name == "ATT" {
        let roll = entry.get_f64("Roll").unwrap_or(0.0);
        let pitch = entry.get_f64("Pitch").unwrap_or(0.0);
        println!("ATT: roll={roll}, pitch={pitch}");
    }
}
```

### Collect into a Vec

```rust
use ardupilot_binlog::File;

let file = File::open("flight.bin")?;
let entries: Vec<_> = file.entries()?.collect::<Result<Vec<_>, _>>()?;

let gps_entries: Vec<_> = entries.iter()
    .filter(|e| e.name == "GPS")
    .collect();

for entry in gps_entries {
    let lat = entry.get_f64("Lat").unwrap_or(0.0) / 1e7;
    let lng = entry.get_f64("Lng").unwrap_or(0.0) / 1e7;
    println!("GPS: {lat}, {lng}");
}
```

### Get time range

```rust
use ardupilot_binlog::File;

let file = File::open("flight.bin")?;
if let Some((first, last)) = file.time_range()? {
    let duration_secs = (last - first) as f64 / 1_000_000.0;
    println!("Flight duration: {duration_secs:.1}s");
}
```

### Read from any `Read` source

```rust
use ardupilot_binlog::Reader;
use std::io::Cursor;

let data: Vec<u8> = /* BIN data from network, embedded resource, etc. */
# vec![];
let mut reader = Reader::new(Cursor::new(data));

for result in &mut reader {
    let entry = result?;
    println!("{}: {} fields", entry.name, entry.len());
}
# Ok::<(), ardupilot_binlog::BinlogError>(())
```

### Inspect discovered formats

```rust
use ardupilot_binlog::File;

let file = File::open("flight.bin")?;
let mut reader = file.entries()?;
for result in reader.by_ref() { result?; }

for (_, fmt) in reader.formats() {
    println!("{}: format='{}', labels={:?}", fmt.name, fmt.format, fmt.labels);
}
```

## Design

- **Synchronous API** — file parsing is CPU-bound sequential work; consumers call from `spawn_blocking()` if needed
- **Minimal dependencies** — only `thiserror`
- **Error recovery** — on corrupted data, scans forward for the next valid message header
- **No hardcoded message types** — all schemas discovered from FMT messages at runtime

## License

MIT OR Apache-2.0
