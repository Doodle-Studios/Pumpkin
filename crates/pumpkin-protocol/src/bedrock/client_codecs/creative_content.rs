//! The client's side of `CCreativeContent`.

use std::io::{Error, ErrorKind, Read};

use crate::{
    bedrock::{
        client::creative_content::{
            CreativeCategory, CreativeGroupInfoPayload, CreativeItemEntryPayload,
        },
        network_item::{NetworkItemDescriptor, read_user_data},
    },
    codec::{var_int::VarInt, var_uint::VarUInt},
    serial::PacketRead,
};

use pumpkin_data::item::BedrockItem;

/// The most groups or entries the packet may claim. Bedrock has about 2,000
/// items, so a count far beyond that means corrupt data.
const MAX_ENTRIES: u32 = 65536;

/// The creative inventory, as the client reads it.
///
/// `CCreativeContent` borrows its groups and entries as slices, so it can't
/// be read into. This holds the same groups and entries, owned. Pumpkin sends
/// it once, at join.
pub struct CreativeContent {
    /// The groups the items are shown in, each with its category and icon.
    pub groups: Vec<CreativeGroupInfoPayload>,
    /// Every item in the creative inventory, by its creative id and group.
    pub entries: Vec<CreativeItemEntryPayload>,
}

fn count<R: Read>(reader: &mut R, what: &str) -> Result<usize, Error> {
    let count = VarUInt::read(reader)?.0;
    if count > MAX_ENTRIES {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("creative {what} count {count} exceeds limit of {MAX_ENTRIES}"),
        ));
    }
    Ok(count as usize)
}

/// Mirrors `PacketWrite for CCreativeContent`: a `VarUInt` count of groups and
/// the groups, then a `VarUInt` count of entries and the entries.
impl PacketRead for CreativeContent {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let groups = (0..count(reader, "group")?)
            .map(|_| CreativeGroupInfoPayload::read(reader))
            .collect::<Result<_, _>>()?;
        let entries = (0..count(reader, "entry")?)
            .map(|_| CreativeItemEntryPayload::read(reader))
            .collect::<Result<_, _>>()?;
        Ok(Self { groups, entries })
    }
}

/// Mirrors `PacketWrite for CreativeCategory`: a byte, one of the five
/// categories the writer can send (it refuses `All` and `Undefined`).
impl PacketRead for CreativeCategory {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        match u8::read(reader)? {
            1 => Ok(Self::Construction),
            2 => Ok(Self::Nature),
            3 => Ok(Self::Equipment),
            4 => Ok(Self::Items),
            5 => Ok(Self::ItemCommandOnly),
            other => Err(Error::new(
                ErrorKind::InvalidData,
                format!("invalid creative category {other}"),
            )),
        }
    }
}

/// Mirrors `NetworkItemDescriptor::write_item_instance`: the id as a `VarInt`,
/// the stack size, the aux value, the block runtime id as a `VarInt`, then the
/// user data with a `VarUInt` length. An item with id 0 has no user data.
fn read_item_instance<R: Read>(reader: &mut R) -> Result<NetworkItemDescriptor, Error> {
    let id = VarInt::read(reader)?;
    let stack_size = u16::read(reader)?;
    let aux_value = VarUInt::read(reader)?;
    let block_runtime_id = VarInt::read(reader)?;
    let user_data_len = VarUInt::read(reader)?.0 as usize;
    if user_data_len > 1_048_576 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "user_data_len exceeds 1MB limit",
        ));
    }
    let mut user_data = vec![0; user_data_len];
    reader.read_exact(&mut user_data)?;
    let (nbt_data, place_on_blocks, destroy_blocks, shield_blocking_tick) =
        read_user_data(user_data, id.0 == i32::from(BedrockItem::SHIELD.id))?;
    Ok(NetworkItemDescriptor {
        id,
        stack_size,
        aux_value,
        block_runtime_id,
        nbt_data,
        place_on_blocks,
        destroy_blocks,
        shield_blocking_tick,
    })
}

/// Mirrors `PacketWrite for CreativeGroupInfoPayload`: the category, the name,
/// then the icon as an item instance.
impl PacketRead for CreativeGroupInfoPayload {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        Ok(Self {
            creative_category: CreativeCategory::read(reader)?,
            name: String::read(reader)?,
            group_icon_item: read_item_instance(reader)?,
        })
    }
}

/// Mirrors `PacketWrite for CreativeItemEntryPayload`: the creative id, the
/// item as an item instance, then the group's index.
impl PacketRead for CreativeItemEntryPayload {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        Ok(Self {
            id: VarUInt::read(reader)?,
            item: read_item_instance(reader)?,
            group_index: VarUInt::read(reader)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bedrock::client::creative_content::CCreativeContent, serial::PacketWrite};

    fn item(id: i32, block_runtime_id: i32) -> NetworkItemDescriptor {
        NetworkItemDescriptor {
            id: VarInt(id),
            stack_size: 1,
            aux_value: VarUInt(0),
            block_runtime_id: VarInt(block_runtime_id),
            ..NetworkItemDescriptor::default()
        }
    }

    #[test]
    fn groups_and_entries_round_trip() {
        let groups = vec![
            CreativeGroupInfoPayload {
                creative_category: CreativeCategory::Construction,
                name: "itemGroup.name.planks".to_string(),
                group_icon_item: item(5, -1_921_718_966),
            },
            CreativeGroupInfoPayload {
                creative_category: CreativeCategory::ItemCommandOnly,
                name: String::new(),
                group_icon_item: NetworkItemDescriptor::default(),
            },
        ];
        let entries = vec![
            CreativeItemEntryPayload {
                id: VarUInt(1),
                item: item(5, -1_921_718_966),
                group_index: VarUInt(0),
            },
            CreativeItemEntryPayload {
                id: VarUInt(2),
                item: NetworkItemDescriptor {
                    place_on_blocks: vec!["minecraft:stone".to_string()],
                    ..item(355, 0)
                },
                group_index: VarUInt(1),
            },
        ];
        let mut first = Vec::new();
        CCreativeContent {
            groups: &groups,
            entries: &entries,
        }
        .write(&mut first)
        .unwrap();

        let mut reader = first.as_slice();
        let read = CreativeContent::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());
        assert_eq!(read.groups.len(), 2);
        assert_eq!(
            read.groups[0].creative_category,
            CreativeCategory::Construction
        );
        assert_eq!(read.groups[0].name, "itemGroup.name.planks");
        assert_eq!(read.groups[0].group_icon_item.id, VarInt(5));
        assert_eq!(
            read.groups[1].creative_category,
            CreativeCategory::ItemCommandOnly
        );
        assert_eq!(read.entries[1].id, VarUInt(2));
        assert_eq!(read.entries[1].item.id, VarInt(355));
        assert_eq!(read.entries[1].item.place_on_blocks, ["minecraft:stone"]);
        assert_eq!(read.entries[1].group_index, VarUInt(1));

        let mut second = Vec::new();
        CCreativeContent {
            groups: &read.groups,
            entries: &read.entries,
        }
        .write(&mut second)
        .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_a_category_the_writer_refuses() {
        // One group, in category 0 (`All`).
        let bytes = [1, 0];
        let Err(error) = CreativeContent::read(&mut &bytes[..]) else {
            panic!("category 0 was accepted");
        };
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }
}
