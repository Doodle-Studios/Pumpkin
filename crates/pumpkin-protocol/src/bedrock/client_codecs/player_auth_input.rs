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
/// order like the official client. Block actions go as a `bool`, a `VarUInt`
/// count and the actions.
///
/// Item interactions and item stack requests are refused. The official client
/// sends neither inside this packet (the M5 capture): it places with an
/// `InventoryTransaction` and moves items with an `ItemStackRequest`.
impl PacketWrite for SPlayerAuthInput {
    fn write<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        if self.item_interaction.is_some() || self.item_stack_request.is_some() {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "PlayerAuthInput item interactions and item stack requests can't be written",
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

        // Item interaction and item stack request: always absent.
        false.write(writer)?;
        false.write(writer)?;

        self.block_actions.is_some().write(writer)?;
        if let Some(actions) = &self.block_actions {
            VarUInt(actions.len() as u32).write(writer)?;
            for action in actions {
                action.write(writer)?;
            }
        }

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
    use crate::{
        bedrock::server::{item_stack_request::ItemStackRequest, player_action::PlayerActionType},
        serial::PacketRead,
    };

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

    /// The block actions a packet carries, as (action, x, y, z, face).
    fn block_actions(packet: &SPlayerAuthInput) -> Vec<(PlayerActionType, i32, i32, i32, i32)> {
        packet
            .block_actions
            .iter()
            .flatten()
            .map(|action| {
                let at = action.block_pos.0;
                (action.action, at.x, at.y, at.z, action.face.0)
            })
            .collect()
    }

    #[test]
    fn starting_to_break_matches_the_official_client() {
        let packet = golden(include_bytes!(
            "fixtures/player_auth_input-start_destroy.bin"
        ));
        assert!(set_flags(&packet).contains(&35)); // PerformBlockActions
        let actions = block_actions(&packet);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            (PlayerActionType::StartDestroyBlock, 0, 200, 29, 4)
        ));
    }

    #[test]
    fn a_predicted_break_matches_the_official_client() {
        let packet = golden(include_bytes!(
            "fixtures/player_auth_input-predict_destroy.bin"
        ));
        let actions = block_actions(&packet);
        assert!(matches!(
            actions[..],
            [
                (PlayerActionType::ContinueDestroyBlock, 0, 200, 29, 4),
                (PlayerActionType::PredictDestroyBlock, 0, 200, 29, 4),
            ]
        ));
    }

    #[test]
    fn letting_go_matches_the_official_client() {
        let packet = golden(include_bytes!(
            "fixtures/player_auth_input-abort_destroy.bin"
        ));
        assert!(matches!(
            block_actions(&packet)[..],
            [(PlayerActionType::AbortDestroyBlock, 0, 199, 30, 0)]
        ));
    }

    #[test]
    fn a_creative_break_matches_the_official_client() {
        let packet = golden(include_bytes!(
            "fixtures/player_auth_input-creative_destroy.bin"
        ));
        assert!(matches!(
            block_actions(&packet)[..],
            [
                (PlayerActionType::StartDestroyBlock, -4, 199, 30, 1),
                (PlayerActionType::PredictDestroyBlock, -4, 199, 30, 1),
            ]
        ));
    }

    #[test]
    fn item_interactions_are_refused() {
        let mut packet =
            SPlayerAuthInput::read(&mut &include_bytes!("fixtures/player_auth_input-idle.bin")[..])
                .unwrap();
        packet.item_stack_request = Some(ItemStackRequest {
            request_id: VarInt(-1),
            actions: Vec::new(),
            filter_strings: Vec::new(),
            filter_cause: -1,
        });
        let error = packet.write(&mut Vec::new()).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }
}
