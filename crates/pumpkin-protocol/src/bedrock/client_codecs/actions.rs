//! Golden tests for the player's actions whose writers are derives beside
//! their structs, against the official client's packets from the M5 capture.
//!
//! Each fixture is read with Pumpkin's reader and written back with ours, and
//! the bytes must match exactly. Commands and chat carry the player's identity,
//! so their captures stay private: those tests are `#[ignore]`d and read
//! `LIMONITE_M5_CAPTURES`, the capture's folder. Round trips with made-up values
//! cover them in every run.

use std::borrow::Cow;

use pumpkin_util::math::position::BlockPos;
use uuid::Uuid;

use crate::{
    bedrock::server::{
        animate::{AnimateAction, SAnimate},
        block_pick_request::SBlockPickRequest,
        command_request::{CommandOriginData, SCommandRequest},
        interact::{Action, SInteract},
        mob_equipment::SMobEquipment,
        player_action::{PlayerActionType, SPlayerAction},
        respawn::{RespawnState, SRespawn},
        text::{SText, TextPacketType},
    },
    codec::{var_int::VarInt, var_ulong::VarULong},
    serial::{PacketRead, PacketWrite},
};

/// Reads `fixture` with Pumpkin's reader, checks every byte was used, writes
/// the packet with our writer, and checks the bytes match.
fn golden<P: PacketRead + PacketWrite>(fixture: &[u8]) -> P {
    let mut reader = fixture;
    let packet = P::read(&mut reader).unwrap();
    assert!(reader.is_empty(), "{} bytes left over", reader.len());
    let mut written = Vec::new();
    packet.write(&mut written).unwrap();
    assert_eq!(written, fixture);
    packet
}

/// Writes with our writer, reads with Pumpkin's reader, and writes again.
fn round_trip<P: PacketRead + PacketWrite>(packet: &P) -> P {
    let mut first = Vec::new();
    packet.write(&mut first).unwrap();
    let read = golden::<P>(&first);
    let mut second = Vec::new();
    read.write(&mut second).unwrap();
    assert_eq!(first, second);
    read
}

/// A packet from the private M5 capture, by its file name.
fn private_capture(name: &str) -> Vec<u8> {
    let dir = std::env::var("LIMONITE_M5_CAPTURES")
        .expect("set LIMONITE_M5_CAPTURES to the M5 capture's folder");
    std::fs::read(std::path::Path::new(&dir).join(name)).unwrap()
}

#[test]
fn selecting_a_slot_matches_the_official_client() {
    let packet: SMobEquipment = golden(include_bytes!("fixtures/mob_equipment.bin"));
    assert_eq!(packet.entity_runtime_id, VarULong(1));
    assert_eq!(packet.item.id, 5);
    assert_eq!(
        (packet.slot, packet.selected_slot, packet.container_id),
        (1, 1, 0)
    );
}

#[test]
fn opening_the_inventory_matches_the_official_client() {
    let packet: SInteract = golden(include_bytes!("fixtures/interact-open_inventory.bin"));
    assert!(matches!(packet.action, Action::OpenInventory));
    assert_eq!(packet.target_runtime_id, VarULong(1));
    assert!(packet.position.is_none());
}

#[test]
fn an_interact_update_matches_the_official_client() {
    let packet: SInteract = golden(include_bytes!("fixtures/interact-update.bin"));
    assert!(matches!(packet.action, Action::InteractUpdate));
    assert_eq!(packet.target_runtime_id, VarULong(0));
}

#[test]
fn starting_a_placement_matches_the_official_client() {
    let packet: SPlayerAction = golden(include_bytes!(
        "fixtures/player_action-start_item_use_on.bin"
    ));
    assert!(matches!(packet.action, PlayerActionType::StartItemUseOn));
    assert_eq!(packet.block_position.0.y, 199);
    assert_eq!(packet.result_pos.0.y, 200);
    assert_eq!(packet.face, VarInt(1));
}

#[test]
fn ending_a_placement_matches_the_official_client() {
    let packet: SPlayerAction = golden(include_bytes!(
        "fixtures/player_action-stop_item_use_on.bin"
    ));
    assert!(matches!(packet.action, PlayerActionType::StopItemUseOn));
    assert_eq!(packet.result_pos, BlockPos::ZERO);
}

#[test]
fn a_creative_break_matches_the_official_client() {
    let packet: SPlayerAction = golden(include_bytes!(
        "fixtures/player_action-creative_destroy.bin"
    ));
    assert!(matches!(
        packet.action,
        PlayerActionType::CreativeDestroyBlock
    ));
    assert_eq!(packet.block_position.0.x, -4);
}

#[test]
fn respawning_matches_the_official_client() {
    let packet: SPlayerAction = golden(include_bytes!("fixtures/player_action-respawn.bin"));
    assert!(matches!(packet.action, PlayerActionType::Respawn));
    assert_eq!(packet.face, VarInt(-1));

    let packet: SRespawn = golden(include_bytes!("fixtures/respawn.bin"));
    assert_eq!(packet.state, RespawnState::ClientReadyToSpawn);
    assert_eq!(packet.player_runtime_id, VarULong(1));
}

#[test]
fn swinging_matches_the_official_client() {
    for fixture in [
        &include_bytes!("fixtures/animate-swing_build.bin")[..],
        &include_bytes!("fixtures/animate-swing_mine.bin")[..],
    ] {
        let packet: SAnimate = golden(fixture);
        assert_eq!(packet.action, AnimateAction::SwingArm);
        assert_eq!(packet.target_actor_runtime_id, VarULong(1));
    }
}

#[test]
fn picking_a_block_matches_the_official_client() {
    let packet: SBlockPickRequest = golden(include_bytes!("fixtures/block_pick_request.bin"));
    assert_eq!(packet.position.0.z, 28);
    assert!(packet.with_data);
    assert_eq!(packet.max_slots, 9);
}

#[test]
fn a_command_round_trips() {
    let read = round_trip(&SCommandRequest {
        command: Cow::Borrowed("/tp @s 0.5 200 2.5 0 0"),
        origin: CommandOriginData {
            r#type: Cow::Borrowed("player"),
            uuid: Uuid::from_u128(0x1234_5678),
            request_id: Cow::Borrowed(""),
            player_id: 0,
        },
        is_internal: false,
        version: Cow::Borrowed("latest"),
    });
    assert_eq!(read.command, "/tp @s 0.5 200 2.5 0 0");
    assert_eq!(read.origin.r#type, "player");
    assert_eq!(read.origin.uuid, Uuid::from_u128(0x1234_5678));
    assert_eq!(read.version, "latest");
}

#[test]
fn chat_round_trips() {
    let read = round_trip(&SText {
        needs_translation: false,
        r#type: TextPacketType::Chat,
        source_name: Cow::Borrowed("Guest"),
        message: Cow::Borrowed("hello"),
        parameters: Vec::new(),
        xuid: Cow::Borrowed("1000000000000001"),
        platform_chat_id: Cow::Borrowed(""),
        filtered_message: None,
    });
    assert_eq!(read.r#type, TextPacketType::Chat);
    assert_eq!(read.message, "hello");
    assert_eq!(read.xuid, "1000000000000001");
}

#[test]
#[ignore = "reads the private M5 capture from LIMONITE_M5_CAPTURES"]
fn a_command_matches_the_official_client() {
    // The first `/tp` of the session.
    let packet: SCommandRequest = golden(&private_capture("77-00001.bin"));
    assert_eq!(packet.command, "/tp @s 0.5 200 2.5 0 0");
}

#[test]
#[ignore = "reads the private M5 capture from LIMONITE_M5_CAPTURES"]
fn chat_matches_the_official_client() {
    let packet: SText = golden(&private_capture("9-00000.bin"));
    assert_eq!(packet.r#type, TextPacketType::Chat);
}
