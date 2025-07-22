use crc32fast::Hasher;
use rand::random;

#[derive(Clone)]
pub struct Message {
    pub id: u64,
    pub dest: u16,
    pub crc: u32,
    pub data: Vec<u8>,
}

pub struct MessageView<'a> {
    pub id: u64,
    pub dest: u16,
    pub crc: u32,
    pub data: &'a [u8],
}

impl<'a> MessageView<'a> {
    pub fn to_owned(self) -> Message {
        Message {
            id: self.id,
            dest: self.dest,
            crc: self.crc,
            data: self.data.to_vec(),
        }
    }
}


impl Message {
    pub fn new(dest: u16, data: Vec<u8>) -> Self {
        let id = random();
        let mut hasher = Hasher::new();
        hasher.update(&data);
        let crc = hasher.finalize();

        Message {
            id,
            dest,
            crc,
            data,
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(18 + self.data.len());
        
        buffer.extend_from_slice(&self.id.to_le_bytes());
        buffer.extend_from_slice(&self.dest.to_le_bytes());
        buffer.extend_from_slice(&self.crc.to_le_bytes());
        buffer.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&self.data);
        
        buffer
    }

    pub fn deserialize(bytes: &[u8]) -> Result<MessageView, &'static str> {
        if bytes.len() < 18 {
            return Err("Buffer too small for header");
        }

        let id = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let dest = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        let crc = u32::from_le_bytes(bytes[10..14].try_into().unwrap());
        let data_len = u32::from_le_bytes(bytes[14..18].try_into().unwrap()) as usize;
        
        if bytes.len() < 18 + data_len {
            return Err("Buffer too small for data");
        }
        
        Ok(MessageView {
            id,
            dest,
            crc,
            data: &bytes[18..18 + data_len],
        })
    }

    pub const fn serialized_size(&self) -> usize {
        18 + self.data.len()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        let original = Message::new(42, vec![1, 2, 3, 4, 5]);
        let serialized = original.serialize();
        let deserialized = Message::deserialize(&serialized).unwrap();
        
        assert_eq!(original.id, deserialized.id);
        assert_eq!(original.dest, deserialized.dest);
        assert_eq!(original.crc, deserialized.crc);
        assert_eq!(original.data, deserialized.data);
    }


    #[test]
    fn test_zero_copy_deserialize() {
        let original = Message::new(99, vec![100, 200, 255]);
        let serialized = original.serialize();
        let view = Message::deserialize(&serialized).unwrap();
        
        assert_eq!(original.id, view.id);
        assert_eq!(original.dest, view.dest);
        assert_eq!(original.crc, view.crc);
        assert_eq!(original.data, view.data);
        
        let owned = view.to_owned();
        assert_eq!(original.data, owned.data);
    }

    #[test]
    fn test_empty_data() {
        let message = Message::new(0, vec![]);
        let serialized = message.serialize();
        let deserialized = Message::deserialize(&serialized).unwrap();
        
        assert_eq!(message.dest, deserialized.dest);
        assert_eq!(message.data, deserialized.data);
        assert!(deserialized.data.is_empty());
    }

    #[test]
    fn test_serialized_size() {
        let message = Message::new(1, vec![1, 2, 3, 4, 5]);
        assert_eq!(message.serialized_size(), 18 + 5);
        
        let empty_message = Message::new(1, vec![]);
        assert_eq!(empty_message.serialized_size(), 18);
    }
}
