use std::io::{self, Error, Read};

use pumpkin_nbt::compound::NbtCompound;

use crate::{bedrock::client::item_registry::ItemData, codec::var_int::VarInt, serial::PacketRead};

/// A reader that keeps a copy of every byte read through it.
struct Tee<'r, R> {
    inner: &'r mut R,
    bytes: Vec<u8>,
}

impl<R: Read> Read for Tee<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.bytes.extend_from_slice(&buf[..read]);
        Ok(read)
    }
}

/// Mirrors `PacketWrite for ItemData`: the name, the id, whether it's
/// component based, the version, then the component data.
///
/// The component data is a network NBT compound written with no length in
/// front (`#[serial(no_prefix)]`), so the derive can't read it. The compound
/// is parsed to find where it ends, and its bytes are kept as they came.
impl PacketRead for ItemData {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let item_name = String::read(reader)?;
        let item_id = i16::read(reader)?;
        let is_component_based = bool::read(reader)?;
        let item_version = VarInt::read(reader)?;
        let mut tee = Tee {
            inner: reader,
            bytes: Vec::new(),
        };
        NbtCompound::read(&mut tee)?;
        Ok(Self {
            item_name,
            item_id,
            is_component_based,
            item_version,
            component_data: tee.bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_data::item::{BedrockItem, BedrockItemVersion};

    use super::*;
    use crate::{bedrock::client::item_registry::CItemRegistry, serial::PacketWrite};

    /// Pumpkin's whole registry, built as `World::spawn_bedrock_player` does.
    fn pumpkin_registry() -> CItemRegistry {
        CItemRegistry {
            items: BedrockItem::ALL_BEDROCK_ITEMS
                .iter()
                .map(|b| ItemData {
                    item_name: b.registry_key.into(),
                    item_id: b.id,
                    is_component_based: b.component_based,
                    item_version: VarInt::from(match b.version {
                        BedrockItemVersion::Legacy => 0,
                        BedrockItemVersion::DataDriven => 1,
                        BedrockItemVersion::None => 2,
                    }),
                    component_data: b.definition_components.into(),
                })
                .collect(),
        }
    }

    #[test]
    fn pumpkin_registry_round_trips() {
        let registry = pumpkin_registry();
        let mut first = Vec::new();
        registry.write(&mut first).unwrap();
        let mut reader = first.as_slice();
        let read = CItemRegistry::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());

        assert_eq!(read.items.len(), registry.items.len());
        for (read, sent) in read.items.iter().zip(&registry.items) {
            assert_eq!(read.item_name, sent.item_name);
            assert_eq!(read.item_id, sent.item_id);
            assert_eq!(read.is_component_based, sent.is_component_based);
            assert_eq!(read.item_version, sent.item_version);
            assert_eq!(
                read.component_data, sent.component_data,
                "{}",
                sent.item_name
            );
        }

        let mut second = Vec::new();
        read.write(&mut second).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn component_data_ends_where_its_compound_does() {
        // Two items: the first has components, so the reader has to stop at
        // its compound's end to find the second.
        let mut components = NbtCompound::new();
        components.put_int("max_stack_size", 16);
        let mut nbt = Vec::new();
        components.write(&mut nbt).unwrap();
        let registry = CItemRegistry {
            items: vec![
                ItemData {
                    item_name: "limonite:test".to_string(),
                    item_id: -300,
                    is_component_based: true,
                    item_version: VarInt(1),
                    component_data: nbt.clone(),
                },
                ItemData {
                    item_name: "minecraft:dirt".to_string(),
                    item_id: 3,
                    is_component_based: false,
                    item_version: VarInt(2),
                    component_data: b"\n\0\0".to_vec(),
                },
            ],
        };
        let mut bytes = Vec::new();
        registry.write(&mut bytes).unwrap();
        let read = CItemRegistry::read(&mut bytes.as_slice()).unwrap();
        assert_eq!(read.items[0].item_id, -300);
        assert_eq!(read.items[0].component_data, nbt);
        assert_eq!(read.items[1].item_name, "minecraft:dirt");
        assert_eq!(read.items[1].component_data, b"\n\0\0");
    }
}
