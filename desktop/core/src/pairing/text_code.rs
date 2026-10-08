//! The text pairing code: a [`PairingToken`] a human can read and type.
//!
//! ADR-0022 §D5 adds a text mode for devices that cannot show or scan a QR
//! code; `docs/architecture/MULTI-DEVICE-MESH-V2.md` §7 fixes its format:
//!
//! ```text
//! data  = base32(token)                     RFC 4648, A-Z 2-7, no padding: 32 symbols
//! check = base32 symbol of the first five bits of
//!         SHA-256("pliwee/text-code-check/v1" || token)
//! shown = DDDD-DDDD-DDDD-DDDD-DDDD-DDDD-DDDD-DDDD-C
//! ```
//!
//! This module is a codec and nothing else. It opens no connection, creates
//! no trust and knows nothing of pairing windows. Its one promise is that a
//! [`PairingToken`] comes out of [`parse`] only when the input was exactly 33
//! symbols of the alphabet with a matching check symbol, so a mistyped code is
//! refused before anything reaches the network.
//!
//! # The check symbol is not authentication
//!
//! It catches transcription mistakes. A changed check symbol is always
//! refused, because only one check symbol fits a given token. A changed data
//! symbol is a different token whose check symbol is a fresh five bits of
//! SHA-256, so it is refused 31 times in 32, not always. The 160-bit token
//! remains the pairing secret, and the proof and confirmation MACs are what
//! authenticate it.
//!
//! # The code is the token
//!
//! Anyone who reads the code holds the secret. [`TextCode`] is therefore not
//! `Clone`, prints as `<redacted>` and is zeroed on drop, as the token is, and
//! [`TextCodeError`] never repeats any of the input. Neither the code nor the
//! token is ever logged (ADR-0022 §D5, threat T11).
//!
//! The buffers this module owns are zeroed on every path. What it cannot
//! scrub: the caller's input string, the SHA-256 state (as in the proof MAC,
//! `sha2` does not zero it), and stack copies left when a token is moved.

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use super::{PairingToken, TOKEN_LEN};

/// Domain separator of the check symbol's hash.
pub const CHECK_DOMAIN: &[u8] = b"pliwee/text-code-check/v1";

/// RFC 4648 base32 symbols carrying the token: 160 bits at five per symbol.
pub const DATA_SYMBOLS: usize = 32;

/// Data symbols plus the check symbol.
pub const CODE_SYMBOLS: usize = DATA_SYMBOLS + 1;

/// Data symbols per displayed group.
pub const GROUP_LEN: usize = 4;

/// The separator [`render`] puts between groups. Visual only: [`parse`]
/// ignores it, and ASCII spaces, wherever they appear.
pub const SEPARATOR: char = '-';

/// The RFC 4648 base32 alphabet. Index is the symbol's five-bit value.
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

// `render` relies on the token filling the data symbols exactly, with no
// padding bits: every data symbol then carries five bits of the token.
const _: () = assert!(TOKEN_LEN * 8 == DATA_SYMBOLS * 5);

/// Why a typed code was refused. Deliberately carries no part of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextCodeError {
    /// A character other than A-Z, a-z, 2-7, an ASCII space or a hyphen.
    #[error("pairing code contains a character outside the code alphabet")]
    InvalidCharacter,
    /// Not exactly 33 symbols once spaces and hyphens are removed.
    #[error("pairing code has the wrong number of symbols")]
    WrongLength,
    /// The check symbol does not match the data symbols.
    #[error("pairing code check symbol does not match")]
    BadCheckSymbol,
}

/// A rendered text pairing code, ready to be shown on the issuer's screen.
///
/// It is the pairing secret in another form, and is held like one: not
/// `Clone`, not `Display`, redacted by `Debug`, zeroed on drop.
pub struct TextCode(String);

impl TextCode {
    /// The display form, for the screen that shows it and for nothing else.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Drop for TextCode {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for TextCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TextCode(<redacted>)")
    }
}

/// The check symbol of `token`.
pub fn check_symbol(token: &PairingToken) -> char {
    let mut digest = Sha256::new()
        .chain_update(CHECK_DOMAIN)
        .chain_update(token.as_bytes())
        .finalize();
    let symbol = ALPHABET[usize::from(digest[0] >> 3)];
    digest.zeroize();
    char::from(symbol)
}

/// Renders `token` as eight groups of four data symbols and the check
/// symbol, separated by [`SEPARATOR`].
///
/// Takes a [`PairingToken`], never a byte slice: a secret of any other length
/// cannot be rendered.
pub fn render(token: &PairingToken) -> TextCode {
    let mut data = data_encoding::BASE32_NOPAD.encode(token.as_bytes());
    debug_assert_eq!(data.len(), DATA_SYMBOLS);

    let groups = DATA_SYMBOLS / GROUP_LEN + 1;
    let mut out = String::with_capacity(CODE_SYMBOLS + groups - 1);
    for (i, symbol) in data.chars().enumerate() {
        if i > 0 && i.is_multiple_of(GROUP_LEN) {
            out.push(SEPARATOR);
        }
        out.push(symbol);
    }
    out.push(SEPARATOR);
    out.push(check_symbol(token));
    data.zeroize();
    TextCode(out)
}

/// Parses a typed or pasted code back into the token it carries.
///
/// ASCII spaces and hyphens are ignored wherever they appear, and lowercase
/// ASCII letters are read as uppercase. Anything else is refused: another
/// character, fewer or more than 33 symbols, or a check symbol that does not
/// match. The first problem found is the one reported.
///
/// Pure and synchronous: a code that fails here never reaches a socket.
pub fn parse(input: &str) -> Result<PairingToken, TextCodeError> {
    let mut symbols = [0u8; CODE_SYMBOLS];
    let result = normalize(input, &mut symbols).and_then(|()| decode(&symbols));
    symbols.zeroize();
    result
}

/// Copies the symbols of `input` into `out`, uppercased, or says why not.
fn normalize(input: &str, out: &mut [u8; CODE_SYMBOLS]) -> Result<(), TextCodeError> {
    let mut len = 0;
    // Bytes, not chars: every accepted symbol is ASCII, and any byte of a
    // multi-byte UTF-8 sequence is >= 0x80 and so refused below.
    for &byte in input.as_bytes() {
        let symbol = match byte {
            b' ' | b'-' => continue,
            b'A'..=b'Z' | b'2'..=b'7' => byte,
            b'a'..=b'z' => byte.to_ascii_uppercase(),
            _ => return Err(TextCodeError::InvalidCharacter),
        };
        let slot = out.get_mut(len).ok_or(TextCodeError::WrongLength)?;
        *slot = symbol;
        len += 1;
    }
    if len != CODE_SYMBOLS {
        return Err(TextCodeError::WrongLength);
    }
    Ok(())
}

fn decode(symbols: &[u8; CODE_SYMBOLS]) -> Result<PairingToken, TextCodeError> {
    let mut bytes = [0u8; TOKEN_LEN];
    // Every symbol is already in the alphabet and 32 symbols are exactly 20
    // bytes, so this cannot fail; the mapping keeps it from panicking if it
    // ever did.
    let decoded = data_encoding::BASE32_NOPAD.decode_mut(&symbols[..DATA_SYMBOLS], &mut bytes);
    if !matches!(decoded, Ok(TOKEN_LEN)) {
        bytes.zeroize();
        return Err(TextCodeError::InvalidCharacter);
    }
    let token = PairingToken::from_bytes(bytes);
    bytes.zeroize();

    let expected = check_symbol(&token) as u8;
    if !bool::from(expected.ct_eq(&symbols[DATA_SYMBOLS])) {
        return Err(TextCodeError::BadCheckSymbol);
    }
    Ok(token)
}
