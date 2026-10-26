use std::{
    collections::HashMap,
    io::{Error, ErrorKind, Read},
};

use pumpkin_util::math::{position::BlockPos, vector3::Vector3};

use crate::{
    bedrock::client::set_actor_data::{MetadataValue, PropertySyncData, SyncedActorDataList},
    codec::{var_int::VarInt, var_long::VarLong, var_uint::VarUInt},
    serial::PacketRead,
};

/// The most entries a metadata or property map may claim. Bedrock defines
/// about 140 metadata keys, so a count far beyond that means corrupt data.
const MAX_ENTRIES: u32 = 4096;

fn entry_count<R: Read>(reader: &mut R, what: &str) -> Result<u32, Error> {
    let count = VarUInt::read(reader)?.0;
    if count > MAX_ENTRIES {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("{what} count {count} exceeds limit of {MAX_ENTRIES}"),
        ));
    }
    Ok(count)
}

/// Mirrors `PacketWrite for SyncedActorDataList`: a `VarUInt` count, then for
/// each entry its key and its type as `VarUInt`s, the type again as a byte
/// (Pumpkin's format since its 1.26.40 update), and the value.
///
/// A compound tag can't be read, because Pumpkin can't write one either.
impl PacketRead for SyncedActorDataList {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let count = entry_count(reader, "actor data")?;
        let mut entries = HashMap::with_capacity(count.min(64) as usize);
        for _ in 0..count {
            let key = VarUInt::read(reader)?.0;
            let type_id = VarUInt::read(reader)?.0;
            let repeated = u8::read(reader)?;
            if u32::from(repeated) != type_id {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("actor data type byte {repeated} doesn't match type {type_id}"),
                ));
            }
            let value = match type_id {
                0 => MetadataValue::Byte(i8::read(reader)?),
                1 => MetadataValue::Short(i16::read(reader)?),
                2 => MetadataValue::Int(VarInt::read(reader)?.0),
                3 => MetadataValue::Float(f32::read(reader)?),
                4 => MetadataValue::String(String::read(reader)?),
                5 => {
                    return Err(Error::new(
                        ErrorKind::Unsupported,
                        "compound tag actor data can't be read",
                    ));
                }
                6 => MetadataValue::ItemPos(BlockPos::read(reader)?),
                7 => MetadataValue::Int64(VarLong::read(reader)?.0),
                8 => MetadataValue::Vec3(Vector3::<f32>::read(reader)?),
                other => {
                    return Err(Error::new(
                        ErrorKind::InvalidData,
                        format!("unknown actor data type {other}"),
                    ));
                }
            };
            entries.insert(key, value);
        }
        Ok(Self(entries))
    }
}

/// Mirrors `PacketWrite for PropertySyncData`: a `VarUInt` count of integer
/// properties, each a `VarUInt` key and a `VarInt` value, then a count of
/// float properties, each a `VarUInt` key and an `f32`.
impl PacketRead for PropertySyncData {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let ints = entry_count(reader, "integer property")?;
        let mut int_entries_list = HashMap::with_capacity(ints.min(64) as usize);
        for _ in 0..ints {
            let key = VarUInt::read(reader)?.0;
            int_entries_list.insert(key, VarInt::read(reader)?.0);
        }
        let floats = entry_count(reader, "float property")?;
        let mut float_entries_list = HashMap::with_capacity(floats.min(64) as usize);
        for _ in 0..floats {
            let key = VarUInt::read(reader)?.0;
            float_entries_list.insert(key, f32::read(reader)?);
        }
        Ok(Self {
            int_entries_list,
            float_entries_list,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial::PacketWrite;

    /// Whether two values are the same type with the same contents.
    /// `MetadataValue` derives neither `PartialEq` nor `Debug`.
    fn same(a: &MetadataValue, b: &MetadataValue) -> bool {
        match (a, b) {
            (MetadataValue::Byte(a), MetadataValue::Byte(b)) => a == b,
            (MetadataValue::Short(a), MetadataValue::Short(b)) => a == b,
            (MetadataValue::Int(a), MetadataValue::Int(b)) => a == b,
            (MetadataValue::Float(a), MetadataValue::Float(b)) => a == b,
            (MetadataValue::String(a), MetadataValue::String(b)) => a == b,
            (MetadataValue::ItemPos(a), MetadataValue::ItemPos(b)) => a == b,
            (MetadataValue::Int64(a), MetadataValue::Int64(b)) => a == b,
            (MetadataValue::Vec3(a), MetadataValue::Vec3(b)) => a == b,
            _ => false,
        }
    }

    /// Compares the decoded entries, not the bytes: the list is a `HashMap`,
    /// which writes its entries in a different order each time.
    #[test]
    fn actor_data_round_trips_every_type() {
        let mut list = SyncedActorDataList::new();
        list.set(0, MetadataValue::Int64(-1 << 40));
        list.set(4, MetadataValue::String("Steve".to_string()));
        list.set(7, MetadataValue::Short(-300));
        list.set(26, MetadataValue::Byte(-2));
        list.set(38, MetadataValue::Float(0.5));
        list.set(53, MetadataValue::Int(-123_456));
        list.set(
            28,
            MetadataValue::ItemPos(BlockPos(Vector3::new(-5, 70, 12))),
        );
        list.set(118, MetadataValue::Vec3(Vector3::new(0.25, -1.5, 3.0)));

        let mut bytes = Vec::new();
        list.write(&mut bytes).unwrap();
        let mut reader = bytes.as_slice();
        let read = SyncedActorDataList::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());

        assert_eq!(read.0.len(), list.0.len());
        for (key, value) in &list.0 {
            assert!(same(value, &read.0[key]), "key {key} differs");
        }
    }

    #[test]
    fn actor_data_rejects_a_compound_tag() {
        // One entry: key 0, type 5 twice, and no value.
        let bytes = [1, 0, 5, 5];
        let Err(error) = SyncedActorDataList::read(&mut &bytes[..]) else {
            panic!("a compound tag was accepted");
        };
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }

    #[test]
    fn actor_data_rejects_an_oversized_count() {
        let mut bytes = Vec::new();
        VarUInt(MAX_ENTRIES + 1).write(&mut bytes).unwrap();
        let Err(error) = SyncedActorDataList::read(&mut bytes.as_slice()) else {
            panic!("an oversized count was accepted");
        };
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn properties_round_trip() {
        let mut properties = PropertySyncData::default();
        properties.int_entries_list.insert(3, -7);
        properties.int_entries_list.insert(9, 1 << 20);
        properties.float_entries_list.insert(1, 0.75);

        let mut bytes = Vec::new();
        properties.write(&mut bytes).unwrap();
        let mut reader = bytes.as_slice();
        let read = PropertySyncData::read(&mut reader).unwrap();
        assert!(reader.is_empty(), "{} bytes left over", reader.len());
        assert_eq!(read.int_entries_list, properties.int_entries_list);
        assert_eq!(read.float_entries_list, properties.float_entries_list);
    }
}
