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

#[cfg(test)]
mod actions;
mod actor_data;
pub mod creative_content;
mod disconnect;
mod gamerules_changed;
mod inventory_transaction;
mod item_registry;
mod item_stack_request;
mod item_stack_response;
mod login;
mod move_actor_delta;
mod move_player;
mod network_chunk_publisher_update;
mod player_auth_input;
mod resource_pack_client_response;
mod start_game;

pub use creative_content::CreativeContent;

/// Tests for the packets whose client-side codec is a derive next to the struct.
#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use crate::{
        bedrock::{
            client::{
                ActorLink, AttributeData, AttributeModifier, BuildPlatform, CAddActor,
                CAddItemActor, CAddPlayer, CCorrectPlayerMovePrediction, CMoveActorAbsolute,
                CRemoveActor, CSetActorData, CSetActorMotion, CSetHealth, CSetPlayerGameType,
                CSetTime, CTakeItemActor, CUpdateAbilities, CUpdateAttributes,
                CommandPermissionLevel, GameType, PlayerPermissionLevel, SerializedAbilitiesData,
                SerializedAbilitiesDataSerializedLayer, SyncedAttribute,
                chunk_radius_updated::CChunkRadiusUpdated,
                container_open::CContainerOpen,
                inventory_content::CInventoryContent,
                inventory_slot::CInventorySlot,
                play_status::CPlayStatus,
                player_hotbar::CPlayerHotbar,
                set_actor_data::{MetadataValue, PropertySyncData, SyncedActorDataList},
                update_block::CUpdateBlock,
            },
            network_item::{
                ContainerName, FullContainerName, ItemStackWrapper, NetworkItemStackDescriptor,
            },
            server::{
                container_close::SContainerClose, request_chunk_radius::SRequestChunkRadius,
                set_local_player_as_initialized::SSetLocalPlayerAsInitialized,
            },
        },
        codec::{var_int::VarInt, var_long::VarLong, var_uint::VarUInt, var_ulong::VarULong},
        serial::{PacketRead, PacketWrite},
    };
    use pumpkin_util::math::{position::BlockPos, vector2::Vector2, vector3::Vector3};
    use uuid::Uuid;

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

    // Entities (M5). Each metadata map holds one entry, so the bytes round
    // trip exactly: a `HashMap` writes several entries in no fixed order.
    // `actor_data.rs` tests maps with every value type.

    fn one_entry_data(key: u32, value: MetadataValue) -> SyncedActorDataList {
        let mut data = SyncedActorDataList::new();
        data.set(key, value);
        data
    }

    fn one_entry_properties() -> PropertySyncData {
        let mut properties = PropertySyncData::default();
        properties.int_entries_list.insert(2, -9);
        properties.float_entries_list.insert(5, 1.5);
        properties
    }

    fn actor_link() -> ActorLink {
        ActorLink {
            ridden_unique_id: VarLong(-77),
            rider_unique_id: VarLong(88),
            link_type: 1,
            immediate: true,
            rider_initiated: true,
            vehicle_angular_velocity: 0.25,
        }
    }

    #[test]
    fn add_actor_round_trips() {
        let read = round_trip(&CAddActor {
            target_actor_id: VarLong(-42),
            target_runtime_id: VarULong(42),
            actor_type: "minecraft:cow".to_string(),
            position: Vector3::new(-10.5, 64.0, 3.25),
            velocity: Vector3::new(0.1, -0.08, 0.0),
            rotation: Vector2::new(12.0, 270.0),
            y_head_rotation: 271.5,
            y_body_rotation: 268.0,
            attributes_list: vec![SyncedAttribute {
                attribute_name: "minecraft:health".to_string(),
                min_value: 0.0,
                current_value: 7.5,
                max_value: 10.0,
            }],
            actor_data: one_entry_data(0, MetadataValue::Int64(1 << 11)),
            synced_properties: one_entry_properties(),
            actor_links: vec![actor_link()],
        });
        assert_eq!(read.target_actor_id, VarLong(-42));
        assert_eq!(read.target_runtime_id, VarULong(42));
        assert_eq!(read.actor_type, "minecraft:cow");
        assert_eq!(read.position, Vector3::new(-10.5, 64.0, 3.25));
        assert_eq!(read.rotation, Vector2::new(12.0, 270.0));
        assert_eq!((read.y_head_rotation, read.y_body_rotation), (271.5, 268.0));
        assert_eq!(read.attributes_list[0].attribute_name, "minecraft:health");
        assert_eq!(read.attributes_list[0].current_value, 7.5);
        assert!(matches!(
            read.actor_data.0[&0],
            MetadataValue::Int64(flags) if flags == 1 << 11
        ));
        assert_eq!(read.synced_properties.int_entries_list[&2], -9);
        assert_eq!(read.actor_links[0].rider_unique_id, VarLong(88));
        assert!(read.actor_links[0].rider_initiated);
    }

    #[test]
    fn add_player_round_trips() {
        let read = round_trip(&CAddPlayer {
            uuid: Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef),
            player_name: "Guest".to_string(),
            target_runtime_id: VarULong(5),
            platform_chat_id: "chat".to_string(),
            position: Vector3::new(1.5, 65.62, -2.5),
            velocity: Vector3::new(0.0, -0.0784, 0.0),
            rotation: Vector2::new(-10.0, 90.0),
            y_head_rotation: 91.0,
            carried_item: NetworkItemStackDescriptor {
                id: 3,
                stack_size: 16,
                aux_value: VarUInt(0),
                block_runtime_id: VarUInt(12_345),
                extra_data: vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                net_id: NonZero::new(7),
            },
            player_game_type: GameType::Creative,
            entity_data: one_entry_data(4, MetadataValue::String("Guest".to_string())),
            synced_properties: one_entry_properties(),
            abilities_data: SerializedAbilitiesData {
                target_player_raw_id: 5,
                player_permissions: PlayerPermissionLevel::Member,
                command_permissions: CommandPermissionLevel::Any,
                layers: vec![SerializedAbilitiesDataSerializedLayer::default()],
            },
            actor_links: vec![actor_link()],
            device_id: "device".to_string(),
            build_platform: BuildPlatform::Win32,
        });
        assert_eq!(
            read.uuid,
            Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef)
        );
        assert_eq!(read.player_name, "Guest");
        assert_eq!(read.target_runtime_id, VarULong(5));
        assert_eq!(read.carried_item.id, 3);
        assert_eq!(read.carried_item.net_id, NonZero::new(7));
        assert_eq!(read.player_game_type, GameType::Creative);
        assert_eq!(
            read.abilities_data.player_permissions,
            PlayerPermissionLevel::Member
        );
        assert_eq!(read.device_id, "device");
        assert_eq!(read.build_platform, BuildPlatform::Win32);
    }

    #[test]
    fn add_item_actor_round_trips() {
        let read = round_trip(&CAddItemActor {
            target_actor_id: VarLong(-9),
            target_runtime_id: VarULong(9),
            item: ItemStackWrapper {
                id: 3,
                stack_size: 2,
                block_runtime_id: VarInt(12_345),
                ..ItemStackWrapper::default()
            },
            position: Vector3::new(0.5, 70.25, 0.5),
            velocity: Vector3::new(0.01, 0.2, -0.01),
            entity_data: one_entry_data(38, MetadataValue::Float(1.0)),
            is_from_fishing: true,
        });
        assert_eq!(read.target_actor_id, VarLong(-9));
        assert_eq!((read.item.id, read.item.stack_size), (3, 2));
        assert_eq!(read.item.block_runtime_id, VarInt(12_345));
        assert_eq!(read.velocity, Vector3::new(0.01, 0.2, -0.01));
        assert!(read.is_from_fishing);
    }

    #[test]
    fn remove_actor_round_trips() {
        let read = round_trip(&CRemoveActor::new(VarLong(-12_345)));
        assert_eq!(read.target_actor_id, VarLong(-12_345));
    }

    #[test]
    fn move_actor_absolute_round_trips() {
        let read = round_trip(&CMoveActorAbsolute {
            actor_runtime_id: VarULong(31),
            header: CMoveActorAbsolute::FLAG_ON_GROUND | CMoveActorAbsolute::FLAG_TELEPORT,
            position: Vector3::new(-100.25, 12.5, 7.0),
            rotation_x: 10,
            rotation_y: 200,
            rotation_y_head: 201,
        });
        assert_eq!(read.actor_runtime_id, VarULong(31));
        assert_eq!(read.header, 0x03);
        assert_eq!(read.position, Vector3::new(-100.25, 12.5, 7.0));
        assert_eq!(
            (read.rotation_x, read.rotation_y, read.rotation_y_head),
            (10, 200, 201)
        );
    }

    #[test]
    fn take_item_actor_round_trips() {
        let read = round_trip(&CTakeItemActor {
            item_runtime_id: VarULong(9),
            actor_runtime_id: VarULong(1),
        });
        assert_eq!(read.item_runtime_id, VarULong(9));
        assert_eq!(read.actor_runtime_id, VarULong(1));
    }

    #[test]
    fn set_actor_data_round_trips() {
        let read = round_trip(&CSetActorData {
            target_runtime_id: VarULong(42),
            actor_data: one_entry_data(26, MetadataValue::Byte(-2)),
            synced_properties: one_entry_properties(),
            tick: VarULong(600),
        });
        assert_eq!(read.target_runtime_id, VarULong(42));
        assert!(matches!(read.actor_data.0[&26], MetadataValue::Byte(-2)));
        assert_eq!(read.synced_properties.float_entries_list[&5], 1.5);
        assert_eq!(read.tick, VarULong(600));
    }

    // Inventory (M5). `item_registry.rs`, `creative_content.rs` and
    // `item_stack_response.rs` test the mirrors.

    fn stack(id: i16, stack_size: u16, net_id: i32) -> NetworkItemStackDescriptor {
        NetworkItemStackDescriptor {
            id,
            stack_size,
            aux_value: VarUInt(0),
            block_runtime_id: VarUInt(2_186_211_206),
            extra_data: vec![0; 10],
            net_id: NonZero::new(net_id),
        }
    }

    #[test]
    fn inventory_content_round_trips() {
        let read = round_trip(&CInventoryContent {
            container_id: VarUInt(0),
            slots: vec![stack(3, 16, 34), NetworkItemStackDescriptor::default()],
            full_container_name: FullContainerName {
                container_name: ContainerName::Inventory,
                dynamic_id: Some(2),
            },
            storage_item: stack(50, 4, 35),
        });
        assert_eq!(read.container_id, VarUInt(0));
        assert_eq!(read.slots.len(), 2);
        assert_eq!((read.slots[0].id, read.slots[0].stack_size), (3, 16));
        assert_eq!(read.slots[0].net_id, NonZero::new(34));
        assert_eq!(read.slots[1].id, 0);
        assert_eq!(read.full_container_name.dynamic_id, Some(2));
        assert_eq!(read.storage_item.id, 50);
    }

    #[test]
    fn inventory_slot_round_trips() {
        let read = round_trip(&CInventorySlot {
            container_id: VarUInt(119),
            slot: VarUInt(8),
            full_container_name: Some(FullContainerName {
                container_name: ContainerName::HotBar,
                dynamic_id: None,
            }),
            storage_item: Some(stack(5, 1, 0)),
            item: stack(3, 15, 36),
        });
        assert_eq!((read.container_id, read.slot), (VarUInt(119), VarUInt(8)));
        assert_eq!(
            read.full_container_name.map(|name| name.container_name),
            Some(ContainerName::HotBar)
        );
        assert_eq!(read.storage_item.map(|item| item.id), Some(5));
        assert_eq!((read.item.id, read.item.stack_size), (3, 15));

        let read = round_trip(&CInventorySlot {
            container_id: VarUInt(0),
            slot: VarUInt(0),
            full_container_name: None,
            storage_item: None,
            item: NetworkItemStackDescriptor::default(),
        });
        assert!(read.full_container_name.is_none());
        assert!(read.storage_item.is_none());
    }

    #[test]
    fn player_hotbar_round_trips() {
        let read = round_trip(&CPlayerHotbar {
            selected_slot: VarUInt(7),
            container_id: 0x7C,
            should_select_slot: true,
        });
        assert_eq!(read.selected_slot, VarUInt(7));
        assert_eq!(read.container_id, 0x7C);
        assert!(read.should_select_slot);
    }

    #[test]
    fn container_open_round_trips() {
        let read = round_trip(&CContainerOpen {
            container_id: 3,
            container_type: 0xF7,
            position: BlockPos(Vector3::new(-17, 64, 205)),
            target_entity_id: VarLong(-1),
        });
        assert_eq!((read.container_id, read.container_type), (3, 0xF7));
        assert_eq!(read.position, BlockPos(Vector3::new(-17, 64, 205)));
        assert_eq!(read.target_entity_id, VarLong(-1));
    }

    #[test]
    fn container_close_matches_the_official_client() {
        // Pumpkin sends its serverbound struct to clients too, so the same
        // codec reads the server's close and writes ours.
        let fixture = include_bytes!("fixtures/container_close.bin");
        let (packet, written) = read_and_rewrite::<SContainerClose>(fixture);
        assert_eq!((packet.container_id, packet.container_type), (0, 247));
        assert!(!packet.server_initiated_close);
        assert_eq!(written, fixture);
    }
}
