use std::io::{Error, ErrorKind, Write};

use crate::{
    MAX_PACKET_DATA_SIZE, bedrock::server::login::SLogin, codec::var_uint::VarUInt,
    serial::PacketWrite,
};

/// The length of one token, checked against the limit `SLogin::read` enforces.
fn token_len(token: &[u8], what: &str) -> Result<u32, Error> {
    if token.len() > MAX_PACKET_DATA_SIZE {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "{what} length {} exceeds limit {MAX_PACKET_DATA_SIZE}",
                token.len()
            ),
        ));
    }
    u32::try_from(token.len()).map_err(|_| Error::new(ErrorKind::InvalidInput, "token too long"))
}

/// Mirrors `PacketRead for SLogin`.
///
/// The connection request is both tokens, each behind a little-endian `u32`
/// length, and is itself prefixed by its total length as a `VarUInt`. The
/// reader skips that total; the official client sets it to exactly
/// `4 + jwt + 4 + raw_token`, and so does this.
impl PacketWrite for SLogin {
    fn write<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        let jwt_len = token_len(&self.jwt, "JWT")?;
        let raw_token_len = token_len(&self.raw_token, "Raw token")?;
        let connection_request_len = jwt_len
            .checked_add(raw_token_len)
            .and_then(|len| len.checked_add(8))
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "connection request too long"))?;

        self.protocol_version.write_be(writer)?;
        VarUInt(connection_request_len).write(writer)?;
        jwt_len.write(writer)?;
        writer.write_all(&self.jwt)?;
        raw_token_len.write(writer)?;
        writer.write_all(&self.raw_token)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::serial::PacketRead;

    /// Built by hand, in the style of `SLogin`'s own reader test.
    #[test]
    fn writes_the_connection_request_layout() {
        let jwt = br#"{"AuthenticationType":0,"Token":"a.b."}"#.to_vec();
        let raw_token = b"c.d.".to_vec();
        let packet = SLogin {
            protocol_version: 2193,
            jwt: jwt.clone(),
            raw_token: raw_token.clone(),
        };

        let mut expected = Vec::new();
        2193i32.write_be(&mut expected).unwrap();
        VarUInt((4 + jwt.len() + 4 + raw_token.len()) as u32)
            .write(&mut expected)
            .unwrap();
        (jwt.len() as u32).write(&mut expected).unwrap();
        expected.extend_from_slice(&jwt);
        (raw_token.len() as u32).write(&mut expected).unwrap();
        expected.extend_from_slice(&raw_token);

        let mut written = Vec::new();
        packet.write(&mut written).unwrap();
        assert_eq!(written, expected);

        let mut cursor = Cursor::new(written.as_slice());
        let read = SLogin::read(&mut cursor).unwrap();
        assert_eq!(cursor.position() as usize, written.len());
        assert_eq!(read.protocol_version, 2193);
        assert_eq!(read.jwt, jwt);
        assert_eq!(read.raw_token, raw_token);
    }

    /// Golden test against a login the official client sent.
    ///
    /// The capture holds the player's Xbox token and identity, so it never
    /// enters git. Run locally with `LIMONITE_SLOGIN_FIXTURE=<path>` and
    /// `--ignored`.
    #[test]
    #[ignore = "needs a local SLogin capture in LIMONITE_SLOGIN_FIXTURE"]
    fn reproduces_the_official_client_login() {
        let path = std::env::var_os("LIMONITE_SLOGIN_FIXTURE")
            .expect("set LIMONITE_SLOGIN_FIXTURE to a captured SLogin body");
        let fixture = std::fs::read(path).unwrap();

        let mut cursor = Cursor::new(fixture.as_slice());
        let packet = SLogin::read(&mut cursor).unwrap();
        assert_eq!(cursor.position() as usize, fixture.len());

        let mut written = Vec::new();
        packet.write(&mut written).unwrap();
        assert!(
            written == fixture,
            "re-encoded login differs from the capture"
        );
    }
}
