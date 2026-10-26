//! Limonite: lets the NBT reader pull from any [`Read`], taking exactly the compound's own bytes.
//!
//! A packet reader only has [`Read`]. `pumpkin-nbt`'s own source adapter also needs [`io::Seek`],
//! and reading the rest of the packet into a buffer first would swallow every field after the
//! compound, such as the ones after the NBT in `StartGame`'s block properties. Seeking is only
//! used to skip forward, so this adapter reads and discards instead.

use std::{
    borrow::Cow,
    io::{self, ErrorKind, Read},
};

use pumpkin_nbt::deserializer::NbtDataSource;

/// An [`NbtDataSource`] over any [`Read`], which never reads past the compound.
///
/// `pub` here reaches only the rest of this crate, because the module itself is private.
pub struct ReadSource<'r, R>(pub &'r mut R);

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
    use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};

    use crate::serial::{PacketRead, PacketWrite};

    /// Compares the decoded compound, not the re-encoded bytes: `NbtCompound`
    /// keeps its tags in a `HashMap`, whose iteration order differs between
    /// instances, so even Pumpkin's own writer orders the tags differently for
    /// two equal compounds.
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
        assert_eq!(read, compound);

        let mut second = Vec::new();
        read.write(&mut second).unwrap();
        assert_eq!(first.len(), second.len());
    }

    /// A field after the compound is still there for the next reader.
    #[test]
    fn leaves_what_follows_the_compound() {
        let mut compound = NbtCompound::new();
        compound.put_int("count", 3);

        let mut bytes = Vec::new();
        compound.write(&mut bytes).unwrap();
        bytes.extend_from_slice(&[0xAB, 0xCD]);
        let mut reader = bytes.as_slice();
        let read = NbtCompound::read(&mut reader).unwrap();
        assert_eq!(read, compound);
        assert_eq!(reader, [0xAB, 0xCD]);
    }
}
