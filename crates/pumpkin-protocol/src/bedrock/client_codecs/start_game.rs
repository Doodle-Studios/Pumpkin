use std::io::{Error, ErrorKind, Read};

use crate::{
    bedrock::client::start_game::{ExperimentToggle, Experiments},
    serial::PacketRead,
};

/// The same limit `Vec::read` applies.
const MAX_EXPERIMENTS: u32 = 65536;

/// Mirrors `PacketWrite for Experiments`: a little-endian `u32` count (not a
/// `VarUInt`), the toggles, then the "ever toggled" flag.
impl PacketRead for Experiments {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let count = u32::read(reader)?;
        if count > MAX_EXPERIMENTS {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("experiment count {count} exceeds limit of {MAX_EXPERIMENTS}"),
            ));
        }
        let toggles = (0..count)
            .map(|_| ExperimentToggle::read(reader))
            .collect::<Result<Vec<_>, _>>()?;
        let experiments_ever_toggled = bool::read(reader)?;
        Ok(Self {
            toggles,
            experiments_ever_toggled,
        })
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_nbt::compound::NbtCompound;
    use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
    use uuid::Uuid;

    use super::*;
    use crate::{
        bedrock::client::{
            GameType,
            gamerules_changed::{GameRule, RuleValue},
            start_game::{
                BlockProperty, CStartGame, GamePublishSetting, LevelSettings, ServerTelemetryData,
            },
        },
        codec::{var_int::VarInt, var_long::VarLong, var_uint::VarUInt, var_ulong::VarULong},
        serial::PacketWrite,
    };

    /// Built the way `World::spawn_bedrock_player` builds it, plus one block
    /// property, one experiment and one gamerule so every hand-written reader runs.
    #[allow(clippy::too_many_lines)]
    fn start_game() -> CStartGame {
        let mut properties = NbtCompound::new();
        properties.put_int("state", 1);
        let level_settings = LevelSettings {
            seed: 1_790_672_995_490_159_855,
            spawn_biome_type: 0,
            custom_biome_name: String::new(),
            dimension: VarInt(0),
            generator_type: VarInt(1),
            world_gamemode: GameType::Survival,
            hardcore: false,
            difficulty: VarInt(2),
            spawn_position: BlockPos(Vector3::new(4, 64, 34)),
            has_achievements_disabled: true,
            editor_world_type: VarInt(0),
            is_created_in_editor: false,
            is_exported_from_editor: false,
            day_cycle_stop_time: VarInt(-1),
            education_edition_offer: VarUInt(0),
            has_education_features_enabled: false,
            education_product_id: String::new(),
            rain_level: 0.0,
            lightning_level: 0.0,
            has_confirmed_platform_locked_content: false,
            was_multiplayer_intended: true,
            was_lan_broadcasting_intended: true,
            xbox_live_broadcast_setting: GamePublishSetting::Public,
            platform_broadcast_setting: GamePublishSetting::Public,
            commands_enabled: true,
            is_texture_packs_required: false,
            rule_data: vec![GameRule {
                rule_name: "showcoordinates".to_string(),
                rule_can_be_modified: true,
                rule_value: RuleValue::Null,
            }],
            experiments: Experiments {
                toggles: vec![ExperimentToggle {
                    name: "data_driven_items".to_string(),
                    enabled: true,
                }],
                experiments_ever_toggled: true,
            },
            bonus_chest: false,
            has_start_with_map_enabled: false,
            permission_level: 2,
            server_simulation_distance: 10,
            has_locked_behavior_pack: false,
            has_locked_resource_pack: false,
            is_from_locked_world_template: false,
            is_using_msa_gamertags_only: false,
            is_from_world_template: false,
            is_world_template_option_locked: false,
            is_only_spawning_v1_villagers: false,
            is_disabling_personas: false,
            is_disabling_custom_skins: false,
            emote_chat_muted: false,
            game_version: "1.26.51".to_string(),
            limited_world_width: 0,
            limited_world_height: 0,
            new_nether: true,
            edu_shared_uri_button_name: String::new(),
            edu_shared_uri_link_uri: String::new(),
            override_force_experimental_gameplay_has_value: false,
            chat_restriction_level: 0,
            disable_player_interactions: false,
            server_editor_connection_policy: VarInt(0),
            allow_anonymous_block_drops_in_editor_worlds: false,
        };
        CStartGame {
            entity_id: VarLong(1),
            runtime_entity_id: VarULong(1),
            player_gamemode: GameType::Survival,
            position: Vector3::new(4.5, 65.62, 34.5),
            pitch: 0.0,
            yaw: 90.0,
            level_settings,
            level_id: String::new(),
            level_name: "Pumpkin world".to_string(),
            premium_world_template_id: String::new(),
            is_trial: false,
            rewind_history_size: VarInt(0),
            server_authoritative_block_breaking: true,
            current_level_time: 1234,
            enchantment_seed: VarInt(0),
            block_properties: vec![BlockProperty::new("custom:block", properties)],
            multiplayer_correlation_id: Uuid::default().to_string(),
            enable_itemstack_net_manager: true,
            server_version: "Pumpkin Rust Server".to_string(),
            compound_id: 10,
            compound_len: VarUInt(0),
            compound_end: 0,
            block_registry_checksum: 0,
            world_template_id: Uuid::nil(),
            enable_clientside_generation: false,
            blocknetwork_ids_are_hashed: true,
            server_auth_sounds: true,
            server_join_information: None,
            telemetry: ServerTelemetryData {
                server_id: String::new(),
                scenario_id: String::new(),
                world_id: String::new(),
                owner_id: String::new(),
            },
        }
    }

    #[test]
    fn start_game_round_trips() {
        let mut first = Vec::new();
        start_game().write(&mut first).unwrap();

        let mut reader = first.as_slice();
        let read = CStartGame::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());

        assert_eq!(read.runtime_entity_id, VarULong(1));
        assert_eq!(read.level_settings.seed, 1_790_672_995_490_159_855);
        assert_eq!(read.level_settings.rule_data.len(), 1);
        assert_eq!(read.level_settings.experiments.toggles.len(), 1);
        assert_eq!(read.block_properties, start_game().block_properties);

        let mut second = Vec::new();
        read.write(&mut second).unwrap();
        assert_eq!(first, second);
    }
}
