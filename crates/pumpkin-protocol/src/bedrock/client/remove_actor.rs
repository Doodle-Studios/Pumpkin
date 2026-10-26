// Last verified for v2169

#[cfg(feature = "client")]
use crate::serial::PacketRead;
use crate::{codec::var_long::VarLong, serial::PacketWrite};
use pumpkin_macros::packet;

/// Sent by the server to despawn an entity from the client's world.
#[derive(PacketWrite)]
#[cfg_attr(feature = "client", derive(PacketRead))]
#[packet(14)]
pub struct CRemoveActor {
    /// Unique entity ID of the entity to remove.
    pub target_actor_id: VarLong,
}

impl CRemoveActor {
    #[must_use]
    pub const fn new(target_actor_id: VarLong) -> Self {
        Self { target_actor_id }
    }
}
