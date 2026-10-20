use std::io::{Error, Read};

use crate::{bedrock::client::disconnect::CDisconnect, codec::var_int::VarInt, serial::PacketRead};

/// Mirrors `PacketWrite for CDisconnect`: the two messages are only present
/// when `skip_message` is false.
impl PacketRead for CDisconnect {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let reason = VarInt::read(reader)?;
        let skip_message = bool::read(reader)?;
        let (message, filtered_message) = if skip_message {
            (String::new(), String::new())
        } else {
            (String::read(reader)?, String::read(reader)?)
        };
        Ok(Self {
            reason,
            skip_message,
            message,
            filtered_message,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial::PacketWrite;

    fn round_trip(packet: &CDisconnect) -> CDisconnect {
        let mut first = Vec::new();
        packet.write(&mut first).unwrap();
        let mut reader = first.as_slice();
        let read = CDisconnect::read(&mut reader).unwrap();
        assert!(reader.is_empty());

        let mut second = Vec::new();
        read.write(&mut second).unwrap();
        assert_eq!(first, second);
        read
    }

    #[test]
    fn round_trips_with_a_message() {
        let read = round_trip(&CDisconnect::new(2, "Server stopped".to_string()));
        assert_eq!(read.reason, VarInt(2));
        assert!(!read.skip_message);
        assert_eq!(read.message, "Server stopped");
    }

    #[test]
    fn round_trips_without_a_message() {
        let read = round_trip(&CDisconnect::new(0, String::new()));
        assert!(read.skip_message);
        assert!(read.message.is_empty());
    }
}
