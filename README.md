# ardupilot-binlog

Parser for ArduPilot DataFlash BIN log files.

Reads `.bin` / `.BIN` files produced by ArduPilot's onboard DataFlash logger. The format is self-describing — message schemas are discovered from FMT messages within each file, so this crate works with any ArduPilot version without hardcoded message definitions.

## Usage

```toml
[dependencies]
ardupilot-binlog = "0.1"
```

### Parse all entries

```rust
use ardupilot_binlog::File;

let file = File::open("flight.bin")?;
let mut reader = file.entries()?;

while let Some(entry) = reader.next_entry()? {
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
let entries = file.entries()?.collect()?;

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

while let Some(entry) = reader.next_entry()? {
    println!("{}: {} fields", entry.name, entry.fields.len());
}
# Ok::<(), ardupilot_binlog::BinlogError>(())
```

### Inspect discovered formats

```rust
use ardupilot_binlog::File;

let file = File::open("flight.bin")?;
let mut reader = file.entries()?;
while reader.next_entry()?.is_some() {}

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
