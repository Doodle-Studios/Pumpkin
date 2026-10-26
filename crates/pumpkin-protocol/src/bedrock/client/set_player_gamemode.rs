// Last verified for v2169

#[cfg(feature = "client")]
use crate::serial::PacketRead;
use crate::{bedrock::client::GameType, serial::PacketWrite};
use pumpkin_macros::packet;

/// Updates the player's active game mode (e.g. Survival, Creative, Spectator).
#[derive(PacketWrite)]
#[cfg_attr(feature = "client", derive(PacketRead))]
#[packet(62)]
pub struct CSetPlayerGameType {
    /// Target game mode to apply.
    pub player_game_type: GameType,
}
