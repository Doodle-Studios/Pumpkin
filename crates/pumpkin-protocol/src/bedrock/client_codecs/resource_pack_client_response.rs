use std::io::{Error, ErrorKind, Write};

use crate::{
    bedrock::server::resource_pack_client_response::SResourcePackClientResponse,
    codec::var_uint::VarUInt, serial::PacketWrite,
};

/// The status names the official client sends next to each status code.
///
/// Only the two a joining client needs are known, from captures of Bedrock
/// 1.26.52 (`fixtures/`). Refusing packs and requesting downloads aren't
/// supported until their names are captured.
const fn status_name(response: u8) -> Option<&'static str> {
    match response {
        SResourcePackClientResponse::STATUS_HAVE_ALL_PACKS => Some("downloadingfinished"),
        SResourcePackClientResponse::STATUS_COMPLETED => Some("resourcepackstackfinished"),
        _ => None,
    }
}

/// Mirrors `PacketRead for SResourcePackClientResponse`: the status as a
/// `VarUInt` one below the constant, then its name.
///
/// `pack_ids` only follow for `STATUS_SEND_PACKS`, which isn't supported yet.
impl PacketWrite for SResourcePackClientResponse {
    fn write<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        let name = status_name(self.response).ok_or_else(|| {
            Error::new(
                ErrorKind::Unsupported,
                format!(
                    "no known status name for resource pack response {}",
                    self.response
                ),
            )
        })?;
        VarUInt(u32::from(self.response) - 1).write(writer)?;
        name.write(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial::PacketRead;

    fn golden(fixture: &[u8], status: u8) {
        let mut reader = fixture;
        let packet = SResourcePackClientResponse::read(&mut reader).unwrap();
        assert!(reader.is_empty());
        assert_eq!(packet.response, status);

        let mut written = Vec::new();
        packet.write(&mut written).unwrap();
        assert_eq!(written, fixture);
    }

    #[test]
    fn have_all_packs_matches_the_official_client() {
        golden(
            include_bytes!("fixtures/resource_pack_client_response-have_all_packs.bin"),
            SResourcePackClientResponse::STATUS_HAVE_ALL_PACKS,
        );
    }

    #[test]
    fn completed_matches_the_official_client() {
        golden(
            include_bytes!("fixtures/resource_pack_client_response-completed.bin"),
            SResourcePackClientResponse::STATUS_COMPLETED,
        );
    }

    #[test]
    fn statuses_without_a_known_name_are_refused() {
        let packet = SResourcePackClientResponse {
            response: SResourcePackClientResponse::STATUS_REFUSED,
            download_size: 0,
            pack_ids: Vec::new(),
        };
        let error = packet.write(&mut Vec::new()).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }
}
