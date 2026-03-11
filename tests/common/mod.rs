pub fn build_fmt_bootstrap() -> Vec<u8> {
    let mut msg = Vec::new();
    msg.extend_from_slice(&[0xA3, 0x95]);
    msg.push(0x80);
    let mut payload = [0u8; 86];
    payload[0] = 0x80;
    payload[1] = 89;
    payload[2..6].copy_from_slice(b"FMT\0");
    payload[6..11].copy_from_slice(b"BBnNZ");
    let labels = b"Type,Length,Name,Format,Labels";
    payload[22..22 + labels.len()].copy_from_slice(labels);
    msg.extend_from_slice(&payload);
    msg
}

pub fn build_fmt_for_type(
    msg_type: u8,
    msg_len: u8,
    name: &[u8; 4],
    format: &str,
    labels: &str,
) -> Vec<u8> {
    let mut msg = Vec::new();
    msg.extend_from_slice(&[0xA3, 0x95]);
    msg.push(0x80);
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

pub fn build_data_message(msg_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut msg = Vec::new();
    msg.extend_from_slice(&[0xA3, 0x95]);
    msg.push(msg_type);
    msg.extend_from_slice(payload);
    msg
}
