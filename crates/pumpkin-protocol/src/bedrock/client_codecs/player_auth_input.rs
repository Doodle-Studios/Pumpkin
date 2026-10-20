use std::io::{Error, ErrorKind, Write};

use crate::{
    bedrock::server::player_auth_input::SPlayerAuthInput,
    codec::{var_int::VarInt, var_uint::VarUInt},
    serial::PacketWrite,
};

/// How many input flags exist (`Bitset<66>`).
const INPUT_FLAGS: u8 = 66;

/// Mirrors `PacketRead for SPlayerAuthInput`, field by field.
///
/// The input flags go as a count, then each set flag's number, in ascending
/// order like the official client.
///
/// Item interactions, item stack requests and block actions are refused: their
/// writers come with block breaking and placing.
impl PacketWrite for SPlayerAuthInput {
    fn write<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        if self.item_interaction.is_some()
            || self.item_stack_request.is_some()
            || self.block_actions.is_some()
        {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "PlayerAuthInput item and block actions can't be written yet",
            ));
        }

        self.pitch.write(writer)?;
        self.yaw.write(writer)?;
        self.position.write(writer)?;
        self.move_vec.write(writer)?;
        self.head_yaw.write(writer)?;

        let flags: Vec<u8> = (0..INPUT_FLAGS)
            .filter(|&flag| self.input_data.get(flag))
            .collect();
        VarUInt(flags.len() as u32).write(writer)?;
        for flag in flags {
            VarInt(i32::from(flag)).write(writer)?;
        }

        self.input_mode.write(writer)?;
        self.play_mode.write(writer)?;
        self.interaction_model.write(writer)?;
        self.interact_pitch.write(writer)?;
        self.interact_yaw.write(writer)?;
        self.tick.write(writer)?;
        self.delta.write(writer)?;

        // Item interaction, item stack request and block actions: all absent.
        false.write(writer)?;
        false.write(writer)?;
        false.write(writer)?;

        self.vehicle_rotation.write(writer)?;
        self.vehicle_unique_id.write(writer)?;
        self.analog_move.write(writer)?;
        self.camera_orientation.write(writer)?;
        self.raw_move.write(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial::PacketRead;

    /// Reads a capture with Pumpkin's reader, then writes it back with ours.
    fn golden(fixture: &[u8]) -> SPlayerAuthInput {
        let mut reader = fixture;
        let packet = SPlayerAuthInput::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());

        let mut written = Vec::new();
        packet.write(&mut written).unwrap();
        assert_eq!(written, fixture);
        packet
    }

    fn set_flags(packet: &SPlayerAuthInput) -> Vec<u8> {
        (0..INPUT_FLAGS)
            .filter(|&flag| packet.input_data.get(flag))
            .collect()
    }

    #[test]
    fn idle_matches_the_official_client() {
        let packet = golden(include_bytes!("fixtures/player_auth_input-idle.bin"));
        assert_eq!(packet.move_vec.x, 0.0);
        assert_eq!(packet.move_vec.y, 0.0);
        assert_eq!(set_flags(&packet), [48, 50]);
    }

    #[test]
    fn walking_matches_the_official_client() {
        let packet = golden(include_bytes!("fixtures/player_auth_input-walking.bin"));
        assert_eq!(packet.move_vec.y, 1.0);
        assert_eq!(set_flags(&packet), [10, 48, 50]);
    }

    #[test]
    fn jumping_matches_the_official_client() {
        let packet = golden(include_bytes!("fixtures/player_auth_input-jumping.bin"));
        assert_eq!(set_flags(&packet), [3, 6, 16, 31, 48, 60, 61]);
    }

    #[test]
    fn block_actions_are_refused() {
        let mut packet =
            SPlayerAuthInput::read(&mut &include_bytes!("fixtures/player_auth_input-idle.bin")[..])
                .unwrap();
        packet.block_actions = Some(Vec::new());
        let error = packet.write(&mut Vec::new()).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }
}
