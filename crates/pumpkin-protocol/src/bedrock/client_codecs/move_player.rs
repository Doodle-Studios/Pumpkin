use std::io::{Error, Read};

use pumpkin_util::math::vector3::Vector3;

use crate::{
    bedrock::client::move_player::CMovePlayer, codec::var_ulong::VarULong, serial::PacketRead,
};

/// Mirrors `PacketWrite for CMovePlayer`: the fixed fields, then a `bool`
/// saying whether the teleport's cause and source entity type follow. The
/// writer sets it only in teleport mode; without them, both read as 0.
impl PacketRead for CMovePlayer {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let player_runtime_id = VarULong::read(reader)?;
        let position = Vector3::<f32>::read(reader)?;
        let pitch = f32::read(reader)?;
        let yaw = f32::read(reader)?;
        let head_yaw = f32::read(reader)?;
        let mode = u8::read(reader)?;
        let on_ground = bool::read(reader)?;
        let riding_runtime_id = VarULong::read(reader)?;
        let (teleport_cause, teleport_source_entity_type) = if bool::read(reader)? {
            (i32::read(reader)?, i32::read(reader)?)
        } else {
            (0, 0)
        };
        let tick = VarULong::read(reader)?;
        Ok(Self {
            player_runtime_id,
            position,
            pitch,
            yaw,
            head_yaw,
            mode,
            on_ground,
            riding_runtime_id,
            teleport_cause,
            teleport_source_entity_type,
            tick,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial::PacketWrite;

    fn packet(mode: u8) -> CMovePlayer {
        CMovePlayer::new(
            VarULong(7),
            Vector3::new(-1234.5, 70.25, 98_765.75),
            -12.5,
            181.25,
            -90.75,
            mode,
            true,
            VarULong(42),
            3,
            -2,
            VarULong(123_456),
        )
    }

    /// Writes with Pumpkin's writer, reads with ours, and writes again.
    fn round_trip(packet: &CMovePlayer) -> CMovePlayer {
        let mut first = Vec::new();
        packet.write(&mut first).unwrap();
        let mut reader = first.as_slice();
        let read = CMovePlayer::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());

        let mut second = Vec::new();
        read.write(&mut second).unwrap();
        assert_eq!(first, second);
        read
    }

    #[test]
    fn a_teleport_round_trips_with_its_cause() {
        let read = round_trip(&packet(CMovePlayer::MODE_TELEPORT));
        assert_eq!(read.player_runtime_id, VarULong(7));
        assert_eq!(read.position, Vector3::new(-1234.5, 70.25, 98_765.75));
        assert_eq!(
            (read.pitch, read.yaw, read.head_yaw),
            (-12.5, 181.25, -90.75)
        );
        assert_eq!(read.mode, CMovePlayer::MODE_TELEPORT);
        assert!(read.on_ground);
        assert_eq!(read.riding_runtime_id, VarULong(42));
        assert_eq!(
            (read.teleport_cause, read.teleport_source_entity_type),
            (3, -2)
        );
        assert_eq!(read.tick, VarULong(123_456));
    }

    #[test]
    fn a_normal_move_carries_no_teleport_fields() {
        let read = round_trip(&packet(CMovePlayer::MODE_NORMAL));
        assert_eq!(read.mode, CMovePlayer::MODE_NORMAL);
        assert_eq!(
            (read.teleport_cause, read.teleport_source_entity_type),
            (0, 0)
        );
        assert_eq!(read.tick, VarULong(123_456));
    }
}
