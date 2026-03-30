use std::io::{Read, Write};
use std::path::Path;

struct Segment {
    filename: Path,
}

#[derive(Debug, PartialEq, Eq)]
struct SegmentEntry {
    sequence_number: u64,
    message_id: uuid::Uuid,
    flags: u8,
    payload_length: u32,
    payload: Vec<u8>,
}

impl SegmentEntry {
    fn new(sequence_number: u64, message_id: uuid::Uuid, flags: u8, payload: Vec<u8>) -> Self {
        Self {
            sequence_number,
            message_id,
            flags,
            payload_length: payload.len() as u32,
            payload,
        }
    }

    fn write_into<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&self.sequence_number.to_le_bytes())?;
        writer.write_all(self.message_id.as_bytes())?;
        writer.write_all(&[self.flags])?;
        writer.write_all(&self.payload_length.to_le_bytes())?;
        writer.write_all(&self.payload)?;
        Ok(())
    }

    fn read_from<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut seq_buf = [0u8; 8];
        reader.read_exact(&mut seq_buf)?;
        let sequence_number = u64::from_le_bytes(seq_buf);

        let mut uuid_buf = [0u8; 16];
        reader.read_exact(&mut uuid_buf)?;
        let message_id = uuid::Uuid::from_bytes(uuid_buf);

        let mut flags_buf = [0u8; 1];
        reader.read_exact(&mut flags_buf)?;
        let flags = flags_buf[0];

        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf)?;
        let payload_length = u32::from_le_bytes(len_buf);

        let mut payload = vec![0u8; payload_length as usize];
        reader.read_exact(&mut payload)?;

        Ok(Self {
            sequence_number,
            message_id,
            flags,
            payload_length,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use std::io::Cursor;

    #[test]
    fn test_write_and_read_roundtrip() {
        let entry = SegmentEntry {
            sequence_number: 123456789,
            message_id: Uuid::new_v4(),
            flags: 0b10101010,
            payload_length: 5,
            payload: vec![10, 20, 30, 40, 50],
        };

        // Write into a buffer
        let mut buf = Vec::new();
        entry.write_into(&mut buf).expect("write_into failed");

        // Read from buffer
        let mut cursor = Cursor::new(buf);
        let decoded = SegmentEntry::read_from(&mut cursor).expect("read_from failed");

        // Assert all fields match
        assert_eq!(decoded.sequence_number, entry.sequence_number);
        assert_eq!(decoded.message_id, entry.message_id);
        assert_eq!(decoded.flags, entry.flags);
        assert_eq!(decoded.payload_length, entry.payload_length);
        assert_eq!(decoded.payload, entry.payload);
    }

    #[test]
    fn test_empty_payload() {
        let entry = SegmentEntry {
            sequence_number: 0,
            message_id: Uuid::new_v4(),
            flags: 0,
            payload_length: 0,
            payload: vec![],
        };

        let mut buf = Vec::new();
        entry.write_into(&mut buf).expect("write_into failed");

        let mut cursor = Cursor::new(buf);
        let decoded = SegmentEntry::read_from(&mut cursor).expect("read_from failed");

        assert_eq!(decoded.sequence_number, entry.sequence_number);
        assert_eq!(decoded.message_id, entry.message_id);
        assert_eq!(decoded.flags, entry.flags);
        assert_eq!(decoded.payload_length, entry.payload_length);
        assert_eq!(decoded.payload, entry.payload);
    }

    #[test]
    fn test_large_payload() {
        let payload = (0..1024).map(|x| (x % 256) as u8).collect::<Vec<u8>>();
        let entry = SegmentEntry {
            sequence_number: 42,
            message_id: Uuid::new_v4(),
            flags: 0xFF,
            payload_length: payload.len() as u32,
            payload,
        };

        let mut buf = Vec::new();
        entry.write_into(&mut buf).expect("write_into failed");

        let mut cursor = Cursor::new(buf);
        let decoded = SegmentEntry::read_from(&mut cursor).expect("read_from failed");

        assert_eq!(decoded.sequence_number, entry.sequence_number);
        assert_eq!(decoded.message_id, entry.message_id);
        assert_eq!(decoded.flags, entry.flags);
        assert_eq!(decoded.payload_length, entry.payload_length);
        assert_eq!(decoded.payload, entry.payload);
    }
}