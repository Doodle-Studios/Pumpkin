//! The client side of the Bedrock codecs, behind the `client` feature.
//!
//! Limonite: Pumpkin's packets only go one way. The server reads serverbound
//! packets and writes clientbound ones. A client that embeds this crate needs
//! the opposite, so this module adds it:
//! - **writers** for the packets a client sends;
//! - **readers** for the packets it receives.
//!
//! Simple structs get a `#[cfg_attr(feature = "client", derive(..))]` next to
//! their definition. The types whose existing codec is hand-written get a
//! hand-written mirror here, which follows that codec line by line.
//!
//! Only the packets the Limonite client uses so far are covered; each later
//! milestone adds the ones it needs. Sent packets are tested against bytes the
//! official Bedrock client sent (`fixtures/`), and read packets by a round
//! trip through Pumpkin's own writer.

mod disconnect;
mod gamerules_changed;
mod login;
mod move_player;
mod network_chunk_publisher_update;
mod player_auth_input;
mod resource_pack_client_response;
mod start_game;

/// Tests for the packets whose client-side codec is a derive next to the struct.
#[cfg(test)]
mod tests {
    use crate::{
        bedrock::{
            client::{
                AttributeData, AttributeModifier, CCorrectPlayerMovePrediction, CSetActorMotion,
                CSetHealth, CSetPlayerGameType, CSetTime, CUpdateAbilities, CUpdateAttributes,
                CommandPermissionLevel, GameType, PlayerPermissionLevel, SerializedAbilitiesData,
                SerializedAbilitiesDataSerializedLayer, chunk_radius_updated::CChunkRadiusUpdated,
                play_status::CPlayStatus, update_block::CUpdateBlock,
            },
            server::{
                request_chunk_radius::SRequestChunkRadius,
                set_local_player_as_initialized::SSetLocalPlayerAsInitialized,
            },
        },
        codec::{var_int::VarInt, var_uint::VarUInt, var_ulong::VarULong},
        serial::{PacketRead, PacketWrite},
    };
    use pumpkin_util::math::{position::BlockPos, vector2::Vector2, vector3::Vector3};

    /// Reads `fixture` with Pumpkin's reader, checks every byte was used, and
    /// returns the packet with our writer's bytes.
    fn read_and_rewrite<P: PacketRead + PacketWrite>(fixture: &[u8]) -> (P, Vec<u8>) {
        let mut reader = fixture;
        let packet = P::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());
        let mut written = Vec::new();
        packet.write(&mut written).unwrap();
        (packet, written)
    }

    /// Writes with Pumpkin's writer, reads with ours, and writes again.
    fn round_trip<P: PacketRead + PacketWrite>(packet: &P) -> P {
        let mut first = Vec::new();
        packet.write(&mut first).unwrap();
        let (read, second) = read_and_rewrite::<P>(&first);
        assert_eq!(first, second);
        read
    }

    #[test]
    fn request_chunk_radius_matches_the_official_client() {
        let fixture = include_bytes!("fixtures/request_chunk_radius.bin");
        let (packet, written) = read_and_rewrite::<SRequestChunkRadius>(fixture);
        assert_eq!(packet.chunk_radius, VarInt(20));
        assert_eq!(packet.max_chunk_radius, 28);
        assert_eq!(written, fixture);
    }

    #[test]
    fn set_local_player_as_initialized_matches_the_official_client() {
        let fixture = include_bytes!("fixtures/set_local_player_as_initialized.bin");
        let (packet, written) = read_and_rewrite::<SSetLocalPlayerAsInitialized>(fixture);
        assert_eq!(packet.player_id, VarULong(1));
        assert_eq!(written, fixture);
    }

    #[test]
    fn play_status_round_trips() {
        let read = round_trip(&CPlayStatus::PlayerSpawn);
        assert!(matches!(read, CPlayStatus::PlayerSpawn));
        let read = round_trip(&CPlayStatus::LoginSuccess);
        assert!(matches!(read, CPlayStatus::LoginSuccess));
    }

    #[test]
    fn chunk_radius_updated_round_trips() {
        let read = round_trip(&CChunkRadiusUpdated {
            chunk_radius: VarInt(8),
        });
        assert_eq!(read.chunk_radius, VarInt(8));
    }

    #[test]
    fn update_block_round_trips() {
        // Negative x and z, and the water layer: every field away from its default.
        let position = BlockPos(Vector3::new(-1234, 70, -98_765));
        let read = round_trip(&CUpdateBlock::with_layer(position, 0x1234_5678, 1));
        assert_eq!(read.block_position, position);
        assert_eq!(read.block_runtime_id, VarUInt(0x1234_5678));
        assert_eq!(read.flags, VarUInt(0x3));
        assert_eq!(read.layer, VarUInt(1));
    }

    // Player state (M5).

    #[test]
    fn set_actor_motion_round_trips() {
        let read = round_trip(&CSetActorMotion {
            target_runtime_id: VarULong(9),
            motion: Vector3::new(-0.25, 0.5, 1.75),
            tick: VarULong(4321),
        });
        assert_eq!(read.target_runtime_id, VarULong(9));
        assert_eq!(read.motion, Vector3::new(-0.25, 0.5, 1.75));
        assert_eq!(read.tick, VarULong(4321));
    }

    #[test]
    fn correct_player_move_prediction_round_trips() {
        let read = round_trip(&CCorrectPlayerMovePrediction {
            prediction_type: 1,
            pos: Vector3::new(-10.5, 64.0, 7.25),
            pos_delta: Vector3::new(0.1, -0.0784, 0.2),
            rotation: Vector2::new(12.5, -45.0),
            vehicle_angular_velocity: Some(0.75),
            on_ground: true,
            tick: VarULong(999),
        });
        assert_eq!(read.prediction_type, 1);
        assert_eq!(read.pos, Vector3::new(-10.5, 64.0, 7.25));
        assert_eq!(read.pos_delta, Vector3::new(0.1, -0.0784, 0.2));
        assert_eq!(read.rotation, Vector2::new(12.5, -45.0));
        assert_eq!(read.vehicle_angular_velocity, Some(0.75));
        assert!(read.on_ground);
        assert_eq!(read.tick, VarULong(999));
    }

    #[test]
    fn update_abilities_round_trips() {
        let layer = |serialized_layer, walk_speed| SerializedAbilitiesDataSerializedLayer {
            serialized_layer,
            abilities_set: 0x0003_FFFF,
            ability_value: 0x0000_0600,
            fly_speed: 0.05,
            vertical_fly_speed: 1.0,
            walk_speed,
        };
        let read = round_trip(&CUpdateAbilities {
            data: SerializedAbilitiesData {
                target_player_raw_id: -1_234_567_890_123,
                player_permissions: PlayerPermissionLevel::Operator,
                command_permissions: CommandPermissionLevel::Owner,
                layers: vec![layer(1, 0.1), layer(2, 0.2)],
            },
        });
        let data = read.data;
        assert_eq!(data.target_player_raw_id, -1_234_567_890_123);
        assert_eq!(data.player_permissions, PlayerPermissionLevel::Operator);
        assert_eq!(data.command_permissions, CommandPermissionLevel::Owner);
        assert_eq!(data.layers.len(), 2);
        assert_eq!(data.layers[1].serialized_layer, 2);
        assert_eq!(data.layers[1].abilities_set, 0x0003_FFFF);
        assert_eq!(data.layers[1].ability_value, 0x0000_0600);
        assert_eq!(data.layers[1].walk_speed, 0.2);
    }

    #[test]
    fn update_attributes_round_trips() {
        let read = round_trip(&CUpdateAttributes {
            target_runtime_id: VarULong(1),
            attribute_list: vec![AttributeData {
                min_value: 0.0,
                max_value: 1024.0,
                current_value: 0.13,
                default_min_value: 0.0,
                default_max_value: 1024.0,
                default_value: 0.1,
                name: "minecraft:movement".to_string(),
                modifiers: vec![AttributeModifier {
                    id: "sprinting".to_string(),
                    name: "Sprinting speed boost".to_string(),
                    amount: 0.3,
                    operation: 2,
                    operand: 1,
                    is_serializable: true,
                }],
            }],
            tick: VarULong(77),
        });
        assert_eq!(read.target_runtime_id, VarULong(1));
        let attribute = &read.attribute_list[0];
        assert_eq!(attribute.name, "minecraft:movement");
        assert_eq!(attribute.current_value, 0.13);
        assert_eq!(attribute.default_value, 0.1);
        let modifier = &attribute.modifiers[0];
        assert_eq!(modifier.id, "sprinting");
        assert_eq!(
            (modifier.amount, modifier.operation, modifier.operand),
            (0.3, 2, 1)
        );
        assert!(modifier.is_serializable);
        assert_eq!(read.tick, VarULong(77));
    }

    #[test]
    fn set_player_game_type_round_trips() {
        let read = round_trip(&CSetPlayerGameType {
            player_game_type: GameType::Spectator,
        });
        assert_eq!(read.player_game_type, GameType::Spectator);
    }

    #[test]
    fn set_health_round_trips() {
        let read = round_trip(&CSetHealth { health: VarInt(13) });
        assert_eq!(read.health, VarInt(13));
    }

    #[test]
    fn set_time_round_trips() {
        let read = round_trip(&CSetTime::new(-6000));
        assert_eq!(read.time, VarInt(-6000));
    }
}
