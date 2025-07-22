pub struct Chunk {
    pub msg_id: u64,
    pub idx: u16,
    pub total: u16,
    pub data: Vec<u8>,
}

pub struct ChunkView<'a> {
    pub msg_id: u64,
    pub idx: u16,
    pub total: u16,
    pub data: &'a [u8],
}

impl Chunk {
    pub fn new(msg_id: u64, idx: u16, total: u16, data: Vec<u8>) -> Self {
        Chunk {
            msg_id,
            idx,
            total,
            data,
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(16 + self.data.len());

        buffer.extend_from_slice(&self.msg_id.to_le_bytes());
        buffer.extend_from_slice(&self.idx.to_le_bytes());
        buffer.extend_from_slice(&self.total.to_le_bytes());
        buffer.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&self.data);

        buffer
    }

    pub fn deserialize(bytes: &[u8]) -> Result<ChunkView, &'static str> {
        if bytes.len() < 16 {
            return Err("Buffer too small for header");
        }

        let msg_id = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let idx = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        let total = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
        let data_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;

        if bytes.len() < 16 + data_len {
            return Err("Buffer too small for data");
        }

        Ok(ChunkView {
            msg_id,
            idx,
            total,
            data: &bytes[16..16 + data_len],
        })
    }

    pub const fn serialized_size(&self) -> usize {
        16 + self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        let original = Chunk {
            msg_id: 12345678901234567890,
            idx: 42,
            total: 100,
            data: vec![1, 2, 3, 4, 5],
        };
        let serialized = original.serialize();
        let view = Chunk::deserialize(&serialized).unwrap();

        assert_eq!(original.msg_id, view.msg_id);
        assert_eq!(original.idx, view.idx);
        assert_eq!(original.total, view.total);
        assert_eq!(original.data, view.data);
    }

    #[test]
    fn test_empty_data() {
        let chunk = Chunk {
            msg_id: 1,
            idx: 0,
            total: 1,
            data: vec![],
        };
        let serialized = chunk.serialize();
        let view = Chunk::deserialize(&serialized).unwrap();

        assert_eq!(chunk.msg_id, view.msg_id);
        assert_eq!(chunk.data, view.data);
        assert!(view.data.is_empty());
    }

    #[test]
    fn test_buffer_too_small() {
        let partial_data = vec![1, 2, 3, 4, 5];
        assert!(Chunk::deserialize(&partial_data).is_err());
    }

    #[test]
    fn test_serialized_size() {
        let chunk = Chunk {
            msg_id: 1,
            idx: 1,
            total: 1,
            data: vec![1, 2, 3, 4, 5],
        };
        assert_eq!(chunk.serialized_size(), 16 + 5);

        let empty_chunk = Chunk {
            msg_id: 1,
            idx: 1,
            total: 1,
            data: vec![],
        };
        assert_eq!(empty_chunk.serialized_size(), 16);
    }
}
