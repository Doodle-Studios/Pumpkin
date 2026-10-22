use std::io::{Error, ErrorKind, Read};

use pumpkin_util::math::{position::BlockPos, vector2::Vector2};

use crate::{
    bedrock::client::network_chunk_publisher_update::CNetworkChunkPublisherUpdate,
    codec::{var_int::VarInt, var_uint::VarUInt},
    serial::PacketRead,
};

/// The same limit `Vec::read` applies.
const MAX_CACHED_CHUNKS: u32 = 65536;

/// Mirrors `PacketWrite for CNetworkChunkPublisherUpdate`: the centre, the
/// radius in blocks, then a little-endian `u32` count (not a `VarUInt`) of
/// cached chunks, each a pair of `VarInt`s.
impl PacketRead for CNetworkChunkPublisherUpdate {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let pos_for_view = BlockPos::read(reader)?;
        let new_radius = VarUInt::read(reader)?;
        let count = u32::read(reader)?;
        if count > MAX_CACHED_CHUNKS {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("cached chunk count {count} exceeds limit of {MAX_CACHED_CHUNKS}"),
            ));
        }
        let server_build_chunk_list = (0..count)
            .map(|_| {
                let x = VarInt::read(reader)?.0;
                let z = VarInt::read(reader)?.0;
                Ok(Vector2::new(x, z))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Self {
            pos_for_view,
            new_radius,
            server_build_chunk_list,
        })
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_util::math::vector3::Vector3;

    use super::*;
    use crate::serial::PacketWrite;

    #[test]
    fn round_trips_with_cached_chunks() {
        // A negative centre and two cached chunks, so every field is away
        // from its default. Pumpkin itself always sends an empty list.
        let mut packet =
            CNetworkChunkPublisherUpdate::new(BlockPos(Vector3::new(-1234, -60, 98_765)), 8 * 16);
        packet.server_build_chunk_list = vec![Vector2::new(-78, 6172), Vector2::new(3, -1)];

        let mut first = Vec::new();
        packet.write(&mut first).unwrap();
        let mut reader = first.as_slice();
        let read = CNetworkChunkPublisherUpdate::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());

        assert_eq!(read.pos_for_view, packet.pos_for_view);
        assert_eq!(read.new_radius, VarUInt(128));
        assert_eq!(read.server_build_chunk_list, packet.server_build_chunk_list);

        let mut second = Vec::new();
        read.write(&mut second).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_an_oversized_count() {
        let mut bytes = Vec::new();
        CNetworkChunkPublisherUpdate::new(BlockPos(Vector3::new(0, 0, 0)), 0)
            .write(&mut bytes)
            .unwrap();
        // The count is the last four bytes of an empty list.
        let count_at = bytes.len() - 4;
        bytes[count_at..].copy_from_slice(&(MAX_CACHED_CHUNKS + 1).to_le_bytes());
        // The packet doesn't derive `Debug`, so `unwrap_err` isn't available.
        let Err(error) = CNetworkChunkPublisherUpdate::read(&mut bytes.as_slice()) else {
            panic!("an oversized count was accepted");
        };
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }
}
