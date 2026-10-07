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
    use std::io::Cursor;

    use super::*;

    /// A reader holding `prefix` followed by `trailing` bytes. The cursor's
    /// position afterwards says how much `read_envelope` consumed.
    fn reader(prefix: u32, trailing: usize) -> Cursor<Vec<u8>> {
        let mut buf = prefix.to_be_bytes().to_vec();
        buf.resize(4 + trailing, 0);
        Cursor::new(buf)
    }

    #[tokio::test]
    async fn zero_length_prefix_is_a_protocol_error() {
        let mut r = reader(0, 0);
        let err = read_envelope(&mut r)
            .await
            .expect_err("a zero-length frame must be refused");
        assert!(
            matches!(err, Error::Protocol("zero-length frame")),
            "unexpected error: {err:?}"
        );
    }

    #[tokio::test]
    async fn zero_length_prefix_consumes_only_the_prefix() {
        // Bytes after the prefix belong to no frame; they must stay unread.
        let mut r = reader(0, 16);
        let err = read_envelope(&mut r)
            .await
            .expect_err("a zero-length frame must be refused");
        assert!(matches!(err, Error::Protocol(_)), "unexpected error: {err:?}");
        assert_eq!(r.position(), 4, "read past a zero-length prefix");
    }

    #[tokio::test]
    async fn prefix_one_over_the_limit_is_refused_without_a_body() {
        // Only the four length bytes exist. Refusing with `FrameTooLarge`
        // rather than `Closed` shows the body was never asked for.
        let mut r = reader(MAX_FRAME_LEN + 1, 0);
        let err = read_envelope(&mut r)
            .await
            .expect_err("a frame over MAX_FRAME_LEN must be refused");
        assert!(
            matches!(err, Error::FrameTooLarge(len, limit)
                if len == MAX_FRAME_LEN + 1 && limit == MAX_FRAME_LEN),
            "unexpected error: {err:?}"
        );
        assert_eq!(r.position(), 4);
    }

    #[tokio::test]
    async fn prefix_one_over_the_limit_leaves_a_present_body_unread() {
        // The full claimed body is available; it must still not be read.
        let claimed = MAX_FRAME_LEN + 1;
        let mut r = reader(claimed, claimed as usize);
        let err = read_envelope(&mut r)
            .await
            .expect_err("a frame over MAX_FRAME_LEN must be refused");
        assert!(
            matches!(err, Error::FrameTooLarge(len, limit)
                if len == claimed && limit == MAX_FRAME_LEN),
            "unexpected error: {err:?}"
        );
        assert_eq!(r.position(), 4, "read the body of an oversized frame");
    }

    #[tokio::test]
    async fn prefix_at_the_limit_passes_the_length_check() {
        // The boundary itself is allowed: the body is read in full and the
        // failure, for an all-zero body, comes from protobuf decoding.
        let mut r = reader(MAX_FRAME_LEN, MAX_FRAME_LEN as usize);
        let err = read_envelope(&mut r)
            .await
            .expect_err("an all-zero body is not a valid envelope");
        assert!(matches!(err, Error::Decode(_)), "unexpected error: {err:?}");
        assert_eq!(r.position(), 4 + u64::from(MAX_FRAME_LEN));
    }
}
