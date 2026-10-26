// Last verified for v2169

#[cfg(feature = "client")]
use crate::serial::PacketRead;
use crate::{codec::var_int::VarInt, serial::PacketWrite};
use pumpkin_macros::packet;

/// Updates player health (legacy packet; attribute updates are typically preferred).
#[derive(PacketWrite)]
#[cfg_attr(feature = "client", derive(PacketRead))]
#[packet(42)]
pub struct CSetHealth {
    /// New health value assigned to the player.
    pub health: VarInt,
}
