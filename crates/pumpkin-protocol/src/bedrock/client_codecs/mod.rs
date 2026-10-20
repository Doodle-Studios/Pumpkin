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
mod nbt;
mod player_auth_input;
mod resource_pack_client_response;
mod start_game;

/// Tests for the packets whose client-side codec is a derive next to the struct.
#[cfg(test)]
mod tests {
    use crate::{
        bedrock::{
            client::{chunk_radius_updated::CChunkRadiusUpdated, play_status::CPlayStatus},
            server::{
                request_chunk_radius::SRequestChunkRadius,
                set_local_player_as_initialized::SSetLocalPlayerAsInitialized,
            },
        },
        codec::{var_int::VarInt, var_ulong::VarULong},
        serial::{PacketRead, PacketWrite},
    };

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
}
