# Golden fixtures for the client-side Bedrock codecs

Packets sent by the official Bedrock client, used to check that the `client` feature's writers
produce exactly what the real client sends.

- **Client:** official Bedrock 1.26.52 (Windows), protocol 2193.
- **Contents:** each file is one packet body after decompression, with no length prefix and no
  packet header. That is exactly what `PacketRead::read` and `PacketWrite::write` handle.
- **Captured** with a temporary hook in `BedrockClient::process_batch` that was never committed,
  in two sessions:
  - **M2:** 2026-10-20, this fork at `80b19bb` (Limonite), offline mode, on `127.0.0.1:19132` over
    `NetherNet`. Join, stand still, walk forward, jump, quit. No clicks, so no item or block
    actions.
  - **M5:** 2026-10-26, this fork at `443b954`, the same setup, in a world with seed
    `1790672995490159855`. A scripted session of moving, building in survival and creative, the
    hotbar, the inventory, death and respawn.

## From M2

| File | Packet | What it shows |
| --- | --- | --- |
| `resource_pack_client_response-have_all_packs.bin` | 8 | status 3, name `downloadingfinished` |
| `resource_pack_client_response-completed.bin` | 8 | status 4, name `resourcepackstackfinished` |
| `request_chunk_radius.bin` | 69 | radius 20, max 28 |
| `set_local_player_as_initialized.bin` | 113 | runtime id 1 |
| `player_auth_input-idle.bin` | 144 | standing on the ground; flags 48, 50 |
| `player_auth_input-walking.bin` | 144 | walking forward (`move_vec` 0, 1); flags 10, 48, 50 |
| `player_auth_input-jumping.bin` | 144 | first tick of a jump; flags 3, 6, 16, 31, 48, 60, 61 |

## From M5

The player's runtime id is 1 throughout.

| File | Packet | What it shows |
| --- | --- | --- |
| `player_auth_input-start_destroy.bin` | 144 | survival: `StartDestroyBlock` on dirt at (0, 200, 29), face 4; flag 35 |
| `player_auth_input-predict_destroy.bin` | 144 | the dirt broken by hand: `ContinueDestroyBlock` and `PredictDestroyBlock` together |
| `player_auth_input-abort_destroy.bin` | 144 | the button let go while on stone: `AbortDestroyBlock`, face 0 |
| `player_auth_input-creative_destroy.bin` | 144 | creative: `StartDestroyBlock` and `PredictDestroyBlock` in one tick |
| `inventory_transaction-place_on_top.bin` | 30 | dirt placed on top of a block: use item, click block (action 0, trigger 1, prediction 1), with the predicted change to hotbar slot 0 as a legacy action |
| `inventory_transaction-click_air.bin` | 30 | the use item, click air (action 1, face 255) the client sends after each placement |
| `inventory_transaction-place_on_side.bin` | 30 | a torch placed on a block's west face (face 4), from hotbar slot 2 |
| `inventory_transaction-drop.bin` | 30 | Q: a normal transaction, the slot's change and a world-source drop |
| `item_stack_request-take.bin` | 147 | inventory screen: take 1 from hotbar slot 0 to the cursor (request id -3) |
| `item_stack_request-place.bin` | 147 | place 1 from the cursor in hotbar slot 8 (request id -5) |
| `item_stack_request-swap.bin` | 147 | swap the cursor with hotbar slot 2 (request id -9) |
| `mob_equipment.bin` | 31 | hotbar slot 1 selected, holding oak planks |
| `interact-open_inventory.bin` | 33 | E: open inventory, target runtime id 1 |
| `interact-update.bin` | 33 | `InteractUpdate`, target 0, which often comes before `MobEquipment` |
| `player_action-start_item_use_on.bin` | 36 | the start of a placement: clicked block, placed position, face 1 |
| `player_action-stop_item_use_on.bin` | 36 | the end of a placement: placed position, face 0 |
| `player_action-creative_destroy.bin` | 36 | creative: `CreativeDestroyBlock` at (-4, 199, 30), face 1 |
| `player_action-respawn.bin` | 36 | the death screen's Respawn button: `Respawn`, face -1 |
| `animate-swing_build.bin` | 44 | arm swing, source `Build`, on each placement |
| `animate-swing_mine.bin` | 44 | arm swing, source `Mine`, about once a frame while mining |
| `block_pick_request.bin` | 34 | middle click on (0, 200, 28): with data, max slots 9 |
| `container_close.bin` | 47 | the inventory screen closed: container 0, type 247, not by the server |
| `respawn.bin` | 45 | `ClientReadyToSpawn`, position (0, 0, 0) |

## Not here

These packets carry the player's identity, so they are never committed. Their tests are
`#[ignore]`d, and read a local capture from an environment variable:

- **The login** (id 1), with the Xbox token: `LIMONITE_SLOGIN_FIXTURE`.
- **Commands** (id 77), with the player's UUID, and **chat** (id 9), with the Xbox id. The M5
  session's are in `run/captures/m5-official/` in the superproject, which is gitignored.
