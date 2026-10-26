// Last verified for v2169

#[cfg(feature = "client")]
use crate::serial::PacketRead;
use crate::{codec::var_ulong::VarULong, serial::PacketWrite};
use pumpkin_macros::packet;

/// Sent by the server when an entity or player picks up a dropped item, playing the pickup animation.
#[derive(PacketWrite)]
#[cfg_attr(feature = "client", derive(PacketRead))]
#[packet(17)]
pub struct CTakeItemActor {
    /// Runtime entity ID of the item entity being collected.
    pub item_runtime_id: VarULong,
    /// Runtime entity ID of the collector entity that picked up the item.
    pub actor_runtime_id: VarULong,
}
