use std::io::{Error, ErrorKind, Read};

use crate::{
    bedrock::client::gamerules_changed::RuleValue, codec::var_uint::VarUInt, serial::PacketRead,
};

/// Mirrors `PacketWrite for RuleValue`, which only writes `Null` as type 0.
///
/// Any other value type is an error until Pumpkin writes typed values.
impl PacketRead for RuleValue {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Error> {
        match VarUInt::read(reader)?.0 {
            0 => Ok(Self::Null),
            other => Err(Error::new(
                ErrorKind::InvalidData,
                format!("unsupported gamerule value type {other}"),
            )),
        }
    }
}
