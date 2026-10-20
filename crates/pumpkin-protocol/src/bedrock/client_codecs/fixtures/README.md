# Golden fixtures for the client-side Bedrock codecs

Packets sent by the official Bedrock client, used to check that the `client` feature's writers
produce exactly what the real client sends.

- **Client:** official Bedrock 1.26.52 (Windows), protocol 2193.
- **Server:** this fork at `80b19bb` (Limonite), offline mode, on `127.0.0.1:19132` over `NetherNet`.
- **Captured:** 2026-10-20, with a temporary hook in `BedrockClient::process_batch` that was never
  committed.
- **Contents:** each file is one packet body after decompression, with no length prefix and no
  packet header. That is exactly what `PacketRead::read` and `PacketWrite::write` handle.
- **Session:** join, stand still, walk forward, jump, quit. No clicks, so no item or block actions.

| File | Packet | What it shows |
| --- | --- | --- |
| `resource_pack_client_response-have_all_packs.bin` | 8 | status 3, name `downloadingfinished` |
| `resource_pack_client_response-completed.bin` | 8 | status 4, name `resourcepackstackfinished` |
| `request_chunk_radius.bin` | 69 | radius 20, max 28 |
| `set_local_player_as_initialized.bin` | 113 | runtime id 1 |
| `player_auth_input-idle.bin` | 144 | standing on the ground; flags 48, 50 |
| `player_auth_input-walking.bin` | 144 | walking forward (`move_vec` 0, 1); flags 10, 48, 50 |
| `player_auth_input-jumping.bin` | 144 | first tick of a jump; flags 3, 6, 16, 31, 48, 60, 61 |

The login packet (id 1) is not here: it carries the player's Xbox token and identity, so it is
never committed. Its test is `#[ignore]`d and reads a local capture from `LIMONITE_SLOGIN_FIXTURE`.
