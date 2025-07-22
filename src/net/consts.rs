pub const CHUNK_SIZE: usize = 1450;
pub const HEADER_SIZE: usize = 12; // Size of the header in bytes (msg_id: u64, idx: u16, total: u16)
pub const SEND_TIMEOUT: u64 = 200;
