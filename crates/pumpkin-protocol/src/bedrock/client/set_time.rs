// Last verified for v2169

use pumpkin_macros::packet;

#[cfg(feature = "client")]
use crate::serial::PacketRead;
use crate::{codec::var_int::VarInt, serial::PacketWrite};

/// Sent by the server to synchronize the day-night cycle time (in ticks) with the client.
#[derive(PacketWrite)]
#[cfg_attr(feature = "client", derive(PacketRead))]
#[packet(10)]
pub struct CSetTime {
    /// In-game time of day in ticks.
    pub time: VarInt,
}

impl CSetTime {
    #[must_use]
    pub const fn new(time: i32) -> Self {
        Self { time: VarInt(time) }
    }
}
