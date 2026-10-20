// Last verified for v2169

use pumpkin_macros::packet;

#[cfg(feature = "client")]
use crate::serial::PacketWrite;
use crate::{codec::var_int::VarInt, serial::PacketRead};

/// Sent by the client to request a specific chunk render distance from the server.
#[derive(PacketRead, Debug)]
#[cfg_attr(feature = "client", derive(PacketWrite))]
#[packet(69)]
pub struct SRequestChunkRadius {
    /// Requested chunk render radius in chunks.
    pub chunk_radius: VarInt,
    /// Upper limit of chunk render radius requested by the client settings.
    pub max_chunk_radius: u8,
}
