//! Length-prefixed framing for protobuf envelopes.
//!
//! Protobuf is not self-delimiting, so each message is preceded by a 4-byte
//! big-endian length.
//!
//! The length is checked *before* allocating. A peer that claims a 4 GiB
//! frame gets an error and a closed connection, not an allocation. This is
//! the cheapest denial-of-service surface in the whole protocol and it is
//! worth being blunt about: `MAX_FRAME_LEN` is small on purpose. Nothing in
//! this protocol version is large — a battery update is a few dozen bytes —
//! and it can be raised deliberately when a capability actually needs it.

use pliwee_proto::Message;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{Error, Result};

/// 64 KiB. See module docs.
pub const MAX_FRAME_LEN: u32 = 64 * 1024;

pub async fn write_envelope<W: AsyncWrite + Unpin>(
    w: &mut W,
    envelope: &pliwee_proto::v1::Envelope,
) -> Result<()> {
    let body = envelope.encode_to_vec();
    let len: u32 = body
        .len()
        .try_into()
        .map_err(|_| Error::FrameTooLarge(u32::MAX, MAX_FRAME_LEN))?;
    if len > MAX_FRAME_LEN {
        return Err(Error::FrameTooLarge(len, MAX_FRAME_LEN));
    }
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(&body).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_envelope<R: AsyncRead + Unpin>(r: &mut R) -> Result<pliwee_proto::v1::Envelope> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Err(Error::Closed),
        Err(e) => return Err(Error::Io(e)),
    }

    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_LEN {
        return Err(Error::FrameTooLarge(len, MAX_FRAME_LEN));
    }
    if len == 0 {
        return Err(Error::Protocol("zero-length frame"));
    }

    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await.map_err(|e| match e.kind() {
        std::io::ErrorKind::UnexpectedEof => Error::Closed,
        _ => Error::Io(e),
    })?;

    Ok(pliwee_proto::v1::Envelope::decode(&body[..])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A stream holding `prefix` as the length, followed by `body`.
    fn stream(prefix: u32, body: &[u8]) -> Cursor<Vec<u8>> {
        let mut bytes = prefix.to_be_bytes().to_vec();
        bytes.extend_from_slice(body);
        Cursor::new(bytes)
    }

    #[tokio::test]
    async fn zero_length_prefix_is_a_protocol_error() {
        let mut r = stream(0, &[]);
        let err = read_envelope(&mut r)
            .await
            .expect_err("zero length must be refused");
        assert!(
            matches!(err, Error::Protocol("zero-length frame")),
            "unexpected error: {err:?}"
        );
    }

    #[tokio::test]
    async fn zero_length_prefix_consumes_only_the_prefix() {
        let mut r = stream(0, &[0xAA; 16]);
        let err = read_envelope(&mut r)
            .await
            .expect_err("zero length must be refused");
        assert!(
            matches!(err, Error::Protocol(_)),
            "unexpected error: {err:?}"
        );
        assert_eq!(r.position(), 4, "nothing past the prefix may be read");
    }

    /// Only the prefix is present. Reaching for a body would end in `Closed`;
    /// `FrameTooLarge` proves the refusal came from the prefix alone.
    #[tokio::test]
    async fn prefix_one_over_the_limit_is_refused_without_a_body() {
        let mut r = stream(MAX_FRAME_LEN + 1, &[]);
        let err = read_envelope(&mut r)
            .await
            .expect_err("oversized frame must be refused");
        assert!(
            matches!(err, Error::FrameTooLarge(len, limit)
                if len == MAX_FRAME_LEN + 1 && limit == MAX_FRAME_LEN),
            "unexpected error: {err:?}"
        );
    }

    #[tokio::test]
    async fn prefix_one_over_the_limit_leaves_a_present_body_unread() {
        let body = vec![0u8; (MAX_FRAME_LEN + 1) as usize];
        let mut r = stream(MAX_FRAME_LEN + 1, &body);
        let err = read_envelope(&mut r)
            .await
            .expect_err("oversized frame must be refused");
        assert!(
            matches!(err, Error::FrameTooLarge(..)),
            "unexpected error: {err:?}"
        );
        assert_eq!(
            r.position(),
            4,
            "the body must not be read before the check"
        );
    }

    /// The other side of the boundary: `MAX_FRAME_LEN` itself passes the length
    /// check, the whole body is read, and only decoding fails (field number 0
    /// is not valid protobuf).
    #[tokio::test]
    async fn prefix_at_the_limit_passes_the_length_check() {
        let body = vec![0u8; MAX_FRAME_LEN as usize];
        let mut r = stream(MAX_FRAME_LEN, &body);
        let err = read_envelope(&mut r)
            .await
            .expect_err("all-zero body is not an envelope");
        assert!(matches!(err, Error::Decode(_)), "unexpected error: {err:?}");
        assert_eq!(r.position(), 4 + u64::from(MAX_FRAME_LEN));
    }
}
