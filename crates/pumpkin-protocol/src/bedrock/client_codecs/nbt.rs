use std::{
    borrow::Cow,
    io::{self, Error, ErrorKind, Read},
};

use pumpkin_nbt::{
    Nbt, NbtCompound,
    deserializer::{NbtDataSource, NbtReadHelperBedrock},
};

use crate::serial::PacketRead;

/// Mirrors `PacketWrite for NbtCompound`, which writes the compound as a
/// nameless Bedrock network NBT document.
impl PacketRead for NbtCompound {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        let mut helper = NbtReadHelperBedrock::new(ReadSource(reader));
        Nbt::read(&mut helper)
            .map(|nbt| nbt.root_tag)
            .map_err(|err| Error::new(ErrorKind::InvalidData, err))
    }
}

/// Lets the NBT reader pull from any [`Read`].
///
/// `pumpkin-nbt`'s own adapter needs [`io::Seek`] as well, which a packet
/// reader doesn't have. Seeking is only used to skip forward, so this reads
/// and discards instead.
struct ReadSource<'r, R>(&'r mut R);

impl<'a, R: Read> NbtDataSource<'a> for ReadSource<'_, R> {
    fn read_u8(&mut self) -> pumpkin_nbt::deserializer::Result<u8> {
        let mut buf = [0u8; 1];
        self.read_bytes(&mut buf)?;
        Ok(buf[0])
    }

    fn read_bytes(&mut self, buf: &mut [u8]) -> pumpkin_nbt::deserializer::Result<()> {
        self.0
            .read_exact(buf)
            .map_err(pumpkin_nbt::Error::Incomplete)
    }

    fn seek_relative(&mut self, offset: i64) -> pumpkin_nbt::deserializer::Result<()> {
        let skip = u64::try_from(offset).map_err(|_| {
            pumpkin_nbt::Error::Incomplete(io::Error::new(
                ErrorKind::Unsupported,
                "cannot seek backwards in a packet",
            ))
        })?;
        let skipped = io::copy(&mut self.0.by_ref().take(skip), &mut io::sink())
            .map_err(pumpkin_nbt::Error::Incomplete)?;
        if skipped == skip {
            Ok(())
        } else {
            Err(pumpkin_nbt::Error::Incomplete(
                ErrorKind::UnexpectedEof.into(),
            ))
        }
    }

    fn read_string(&mut self, len: usize) -> pumpkin_nbt::deserializer::Result<Cow<'a, str>> {
        let mut buf = vec![0u8; len];
        self.read_bytes(&mut buf)?;
        let string =
            cesu8::from_java_cesu8(&buf).map_err(|_| pumpkin_nbt::Error::Cesu8DecodingError)?;
        Ok(Cow::Owned(string.into_owned()))
    }

    fn read_byte_array(&mut self, len: usize) -> pumpkin_nbt::deserializer::Result<Cow<'a, [i8]>> {
        let mut buf = vec![0u8; len];
        self.read_bytes(&mut buf)?;
        Ok(Cow::Owned(buf.into_iter().map(|b| b as i8).collect()))
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_nbt::tag::NbtTag;

    use super::*;
    use crate::serial::PacketWrite;

    #[test]
    fn round_trips_a_compound() {
        let mut compound = NbtCompound::new();
        compound.put_int("count", 3);
        compound.put_string("name", "stone".to_string());
        compound.put("flag", NbtTag::Byte(1));

        let mut first = Vec::new();
        compound.write(&mut first).unwrap();
        let mut reader = first.as_slice();
        let read = NbtCompound::read(&mut reader).unwrap();
        assert!(reader.is_empty());

        let mut second = Vec::new();
        read.write(&mut second).unwrap();
        assert_eq!(first, second);
    }
}
