use std::io::{Error, Read};

use crate::{
    bedrock::client::move_actor_delta::{
        CMoveActorDelta, MOVE_ACTOR_DELTA_FLAG_FORCE_COMPLETION, MOVE_ACTOR_DELTA_FLAG_FORCE_MOVE,
        MOVE_ACTOR_DELTA_FLAG_FORCE_MOVE_LOCAL_ENTITY, MOVE_ACTOR_DELTA_FLAG_HAS_HEAD_YAW,
        MOVE_ACTOR_DELTA_FLAG_HAS_PITCH, MOVE_ACTOR_DELTA_FLAG_HAS_X, MOVE_ACTOR_DELTA_FLAG_HAS_Y,
        MOVE_ACTOR_DELTA_FLAG_HAS_YAW, MOVE_ACTOR_DELTA_FLAG_HAS_Z,
        MOVE_ACTOR_DELTA_FLAG_ON_GROUND,
    },
    codec::var_ulong::VarULong,
    serial::PacketRead,
};

/// Reads a `bool`, and the value after it when it's `true`, setting `flag`.
/// An absent value reads as its default.
fn optional<R: Read, T: PacketRead + Default>(
    reader: &mut R,
    flags: &mut u16,
    flag: u16,
) -> Result<T, Error> {
    if bool::read(reader)? {
        *flags |= flag;
        T::read(reader)
    } else {
        Ok(T::default())
    }
}

/// Mirrors `PacketWrite for CMoveActorDelta`: the runtime id; then x, y, z,
/// pitch, yaw and head yaw, each a `bool` and, when it's `true`, the value;
/// then four `bool`s for on ground, force move, force move local entity and
/// force completion; then the tick.
///
/// The `flags` field is rebuilt from those `bool`s, and an absent field reads
/// as 0.
impl PacketRead for CMoveActorDelta {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let entity_runtime_id = VarULong::read(reader)?;
        let mut flags = 0;
        let x = optional(reader, &mut flags, MOVE_ACTOR_DELTA_FLAG_HAS_X)?;
        let y = optional(reader, &mut flags, MOVE_ACTOR_DELTA_FLAG_HAS_Y)?;
        let z = optional(reader, &mut flags, MOVE_ACTOR_DELTA_FLAG_HAS_Z)?;
        let pitch = optional(reader, &mut flags, MOVE_ACTOR_DELTA_FLAG_HAS_PITCH)?;
        let yaw = optional(reader, &mut flags, MOVE_ACTOR_DELTA_FLAG_HAS_YAW)?;
        let head_yaw = optional(reader, &mut flags, MOVE_ACTOR_DELTA_FLAG_HAS_HEAD_YAW)?;
        for flag in [
            MOVE_ACTOR_DELTA_FLAG_ON_GROUND,
            MOVE_ACTOR_DELTA_FLAG_FORCE_MOVE,
            MOVE_ACTOR_DELTA_FLAG_FORCE_MOVE_LOCAL_ENTITY,
            MOVE_ACTOR_DELTA_FLAG_FORCE_COMPLETION,
        ] {
            if bool::read(reader)? {
                flags |= flag;
            }
        }
        let tick = VarULong::read(reader)?;
        Ok(Self {
            entity_runtime_id,
            flags,
            x,
            y,
            z,
            pitch,
            yaw,
            head_yaw,
            tick,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial::PacketWrite;

    /// Writes with Pumpkin's writer, reads with ours, and writes again.
    fn round_trip(packet: &CMoveActorDelta) -> CMoveActorDelta {
        let mut first = Vec::new();
        packet.write(&mut first).unwrap();
        let mut reader = first.as_slice();
        let read = CMoveActorDelta::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());

        let mut second = Vec::new();
        read.write(&mut second).unwrap();
        assert_eq!(first, second);
        read
    }

    #[test]
    fn every_field_round_trips() {
        let flags = MOVE_ACTOR_DELTA_FLAG_HAS_X
            | MOVE_ACTOR_DELTA_FLAG_HAS_Y
            | MOVE_ACTOR_DELTA_FLAG_HAS_Z
            | MOVE_ACTOR_DELTA_FLAG_HAS_PITCH
            | MOVE_ACTOR_DELTA_FLAG_HAS_YAW
            | MOVE_ACTOR_DELTA_FLAG_HAS_HEAD_YAW
            | MOVE_ACTOR_DELTA_FLAG_ON_GROUND
            | MOVE_ACTOR_DELTA_FLAG_FORCE_MOVE
            | MOVE_ACTOR_DELTA_FLAG_FORCE_MOVE_LOCAL_ENTITY
            | MOVE_ACTOR_DELTA_FLAG_FORCE_COMPLETION;
        let mut packet =
            CMoveActorDelta::new(VarULong(12), flags, -10.5, 64.25, 3.75, 32, 200, 201);
        packet.tick = VarULong(54_321);

        let read = round_trip(&packet);
        assert_eq!(read.entity_runtime_id, VarULong(12));
        assert_eq!(read.flags, flags);
        assert_eq!((read.x, read.y, read.z), (-10.5, 64.25, 3.75));
        assert_eq!((read.pitch, read.yaw, read.head_yaw), (32, 200, 201));
        assert_eq!(read.tick, VarULong(54_321));
    }

    #[test]
    fn absent_fields_read_as_zero() {
        // Only y and yaw changed, in the air: as Pumpkin's entity tracker
        // sends a falling, turning mob.
        let flags = MOVE_ACTOR_DELTA_FLAG_HAS_Y | MOVE_ACTOR_DELTA_FLAG_HAS_YAW;
        let packet = CMoveActorDelta::new(VarULong(3), flags, 1.0, 70.5, 2.0, 9, 128, 7);

        let read = round_trip(&packet);
        assert_eq!(read.flags, flags);
        assert_eq!((read.x, read.y, read.z), (0.0, 70.5, 0.0));
        assert_eq!((read.pitch, read.yaw, read.head_yaw), (0, 128, 0));
    }
}
