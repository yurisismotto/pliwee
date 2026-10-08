//! Text pairing code: known answers, round trip, normalization and rejection
//! (ADR-0022 §D5, `docs/architecture/MULTI-DEVICE-MESH-V2.md` §7).
//!
//! The known answers were not produced by the code under test. Each digest
//! is the output of coreutils `sha256sum` over the domain and the token,
//! written with bash's `printf` escapes, for example the all-zero token:
//!
//! ```text
//! printf 'pliwee/text-code-check/v1\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00' | sha256sum
//! ```
//!
//! The data symbols were worked by hand from the RFC 4648 §6 bit layout,
//! five bytes to eight symbols, most significant bit first.

use pliwee_core::pairing::text_code::{
    self, TextCode, TextCodeError, CHECK_DOMAIN, CODE_SYMBOLS, DATA_SYMBOLS,
};
use pliwee_core::pairing::{PairingToken, TOKEN_LEN};

struct Vector {
    token: [u8; TOKEN_LEN],
    /// SHA-256(CHECK_DOMAIN || token), hex, as measured by `sha256sum`.
    digest: &'static str,
    rendered: &'static str,
}

impl Vector {
    fn digest_first_byte(&self) -> u8 {
        u8::from_str_radix(&self.digest[..2], 16).expect("hex digest")
    }
}

fn seq() -> [u8; TOKEN_LEN] {
    let mut t = [0u8; TOKEN_LEN];
    for (i, b) in t.iter_mut().enumerate() {
        *b = i as u8;
    }
    t
}

fn vectors() -> Vec<Vector> {
    vec![
        // 0x24 >> 3 = 4 -> 'E'
        Vector {
            token: [0x00; TOKEN_LEN],
            digest: "242430a568ba92c23c91c7098a4b27204b93f880c9a719fa4686a0783ba4a123",
            rendered: "AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-E",
        },
        // 0x80 >> 3 = 16 -> 'Q'
        Vector {
            token: [0xff; TOKEN_LEN],
            digest: "800d48cff577bab120496cd642516e9eab6a3ad2a9d581cd44860afd4ca05ce8",
            rendered: "7777-7777-7777-7777-7777-7777-7777-7777-Q",
        },
        // Bytes 00..13. 0x61 >> 3 = 12 -> 'M'
        Vector {
            token: seq(),
            digest: "61a331a48815915acd4e9fb6e58b27d320f9a3cf0f559d1d37f38e8f93d4545e",
            rendered: "AAAQ-EAYE-AUDA-OCAJ-BIFQ-YDIO-B4IB-CEQT-M",
        },
        // 0x4d >> 3 = 9 -> 'J'
        Vector {
            token: *b"pliwee-text-code-v1!",
            digest: "4d4ded558e2df2c067d21dfa93b3139c1b8b626077fc68387f6bce3f60f547fa",
            rendered: "OBWG-S53F-MUWX-IZLY-OQWW-G33E-MUWX-MMJB-J",
        },
    ]
}

const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// The code with every separator removed: 32 data symbols then the check.
fn compact(rendered: &str) -> String {
    rendered.chars().filter(|&c| c != '-').collect()
}

fn parse_err(input: &str) -> TextCodeError {
    match text_code::parse(input) {
        Ok(_) => panic!("input was accepted"),
        Err(e) => e,
    }
}

#[test]
fn check_domain_is_the_spec_value() {
    assert_eq!(CHECK_DOMAIN, b"pliwee/text-code-check/v1");
    assert_eq!(DATA_SYMBOLS, 32);
    assert_eq!(CODE_SYMBOLS, 33);
}

#[test]
fn known_vectors_encode_deterministically() {
    for v in vectors() {
        let token = PairingToken::from_bytes(v.token);
        let expected_check =
            char::from(ALPHABET.as_bytes()[usize::from(v.digest_first_byte() >> 3)]);
        assert_eq!(text_code::check_symbol(&token), expected_check);
        assert_eq!(v.rendered.chars().last(), Some(expected_check));

        let first = text_code::render(&token);
        let second = text_code::render(&token);
        assert_eq!(first.as_str(), v.rendered);
        assert_eq!(second.as_str(), v.rendered);
    }
}

#[test]
fn rendered_code_is_eight_groups_of_four_and_the_check_symbol() {
    for v in vectors() {
        let code = text_code::render(&PairingToken::from_bytes(v.token));
        let groups: Vec<&str> = code.as_str().split('-').collect();
        assert_eq!(groups.len(), 9);
        assert!(groups[..8].iter().all(|g| g.len() == 4));
        assert_eq!(groups[8].len(), 1);
        assert!(code
            .as_str()
            .chars()
            .all(|c| c == '-' || ALPHABET.contains(c)));
    }
}

#[test]
fn known_vectors_parse_to_the_exact_token() {
    for v in vectors() {
        let token = text_code::parse(v.rendered).expect("known vector parses");
        assert_eq!(token.as_bytes(), &v.token);
    }
}

#[test]
fn encode_then_parse_returns_the_exact_token() {
    for _ in 0..256 {
        let token = PairingToken::generate().expect("generate");
        let code = text_code::render(&token);
        let parsed = text_code::parse(code.as_str()).expect("round trip");
        assert_eq!(parsed.as_bytes(), token.as_bytes());
    }
}

#[test]
fn lowercase_spaces_and_hyphens_normalize() {
    for v in vectors() {
        let compact = compact(v.rendered);
        let spaced: String = v.rendered.replace('-', " ");
        let mixed: String = compact
            .chars()
            .enumerate()
            .flat_map(|(i, c)| {
                let sep = if i % 3 == 0 { " -" } else { "" };
                sep.chars().chain(std::iter::once(c))
            })
            .collect();
        let inputs = [
            v.rendered.to_string(),
            v.rendered.to_ascii_lowercase(),
            compact.clone(),
            compact.to_ascii_lowercase(),
            spaced,
            format!("  {}  ", v.rendered.to_ascii_lowercase()),
            format!("--{compact}--"),
            mixed,
        ];
        for input in &inputs {
            let token = text_code::parse(input).expect("normalized input parses");
            assert_eq!(token.as_bytes(), &v.token);
        }
    }
}

#[test]
fn a_changed_check_symbol_is_always_rejected() {
    for v in vectors() {
        let compact = compact(v.rendered);
        let (data, check) = compact.split_at(DATA_SYMBOLS);
        let mut tried = 0;
        for other in ALPHABET.chars().filter(|c| !check.starts_with(*c)) {
            assert_eq!(
                parse_err(&format!("{data}{other}")),
                TextCodeError::BadCheckSymbol
            );
            tried += 1;
        }
        assert_eq!(tried, 31);
    }
}

/// One fixed substitution per vector: the first data symbol, moved one place
/// along the alphabet. These four are refused. Not every data substitution
/// can be: a five-bit check lets about 1 in 32 through, which the next test
/// counts.
#[test]
fn these_changed_data_symbols_are_rejected() {
    for v in vectors() {
        let mut symbols: Vec<char> = compact(v.rendered).chars().collect();
        let pos = ALPHABET.find(symbols[0]).expect("in alphabet");
        symbols[0] = ALPHABET.as_bytes()[(pos + 1) % 32] as char;
        let changed: String = symbols.into_iter().collect();
        assert_eq!(parse_err(&changed), TextCodeError::BadCheckSymbol);
    }
}

/// Every single-symbol substitution of every data symbol, counted.
///
/// A substituted data symbol is a different 160-bit token, so a five-bit
/// check matches it about once in 32. Those are the only acceptances there
/// may be: each must decode to the token the substituted code actually
/// spells, never to the original. The exact counts are pinned so that a
/// change to the check construction cannot pass unnoticed.
#[test]
fn single_data_symbol_substitutions_are_counted_exactly() {
    let mut counts = Vec::new();
    for v in vectors() {
        let original: Vec<char> = compact(v.rendered).chars().collect();
        let (mut rejected, mut accepted) = (0u32, 0u32);
        for pos in 0..DATA_SYMBOLS {
            for other in ALPHABET.chars().filter(|&c| c != original[pos]) {
                let mut symbols = original.clone();
                symbols[pos] = other;
                let input: String = symbols.iter().collect();
                match text_code::parse(&input) {
                    Err(e) => {
                        assert_eq!(e, TextCodeError::BadCheckSymbol);
                        rejected += 1;
                    }
                    Ok(token) => {
                        assert_ne!(token.as_bytes(), &v.token);
                        assert_eq!(compact(text_code::render(&token).as_str()), input);
                        accepted += 1;
                    }
                }
            }
        }
        assert_eq!(rejected + accepted, 32 * 31);
        counts.push(accepted);
    }
    // A regression pin, not an independent vector: measured by running this
    // codec on the four vectors above, 992 substitutions each, 126 accepted
    // in 3968, about 1 in 31.5.
    assert_eq!(counts, vec![19, 39, 30, 38]);
}

#[test]
fn wrong_length_is_rejected() {
    let compact = compact(vectors()[2].rendered);
    let inputs = [
        String::new(),
        "   ---  ".to_string(),
        compact[..DATA_SYMBOLS].to_string(),
        compact[..CODE_SYMBOLS - 2].to_string(),
        format!("{compact}A"),
        format!("{compact}{compact}"),
        "A".repeat(4096),
    ];
    for input in &inputs {
        assert_eq!(
            parse_err(input),
            TextCodeError::WrongLength,
            "{}",
            input.len()
        );
    }
}

#[test]
fn characters_outside_the_alphabet_are_rejected() {
    let rendered = vectors()[2].rendered;
    // 0, 1, 8 and 9 are not RFC 4648 base32; nor is padding, any other
    // punctuation, any whitespace but the ASCII space, or any non-ASCII
    // letter, even one that looks like a valid symbol.
    let bad = [
        "0", "1", "8", "9", "=", "_", ".", "+", "/", "\t", "\n", "\r", "\0", "\u{a0}", "É", "Ａ",
        "Α", "\u{2010}", "\u{2212}",
    ];
    for b in bad {
        for at in [0, 5, rendered.len()] {
            let input = format!("{}{b}{}", &rendered[..at], &rendered[at..]);
            assert_eq!(
                parse_err(&input),
                TextCodeError::InvalidCharacter,
                "{:?} at {at}",
                b.escape_unicode().to_string()
            );
        }
        // Replacing a symbol, so the length is right and only the
        // character is wrong.
        let input = format!("{b}{}", &rendered[1..]);
        assert_eq!(parse_err(&input), TextCodeError::InvalidCharacter);
    }
}

#[test]
fn neither_the_code_nor_an_error_ever_prints_the_secret() {
    let v = &vectors()[3];
    let code = text_code::render(&PairingToken::from_bytes(v.token));
    let debug = format!("{code:?}");
    assert_eq!(debug, "TextCode(<redacted>)");
    assert!(!debug.contains(&compact(v.rendered)[..4]));

    for e in [
        TextCodeError::InvalidCharacter,
        TextCodeError::WrongLength,
        TextCodeError::BadCheckSymbol,
    ] {
        for rendered in [e.to_string(), format!("{e:?}")] {
            assert!(!rendered.contains(&compact(v.rendered)[..4]));
        }
    }
}

/// The public codec only takes a secret of exactly 160 bits: a
/// [`PairingToken`] or a `[u8; TOKEN_LEN]`, never a slice. These bindings
/// stop compiling if any signature is widened.
#[test]
fn public_api_takes_only_a_fixed_length_secret() {
    let _: fn([u8; TOKEN_LEN]) -> PairingToken = PairingToken::from_bytes;
    let _: fn(&PairingToken) -> TextCode = text_code::render;
    let _: fn(&PairingToken) -> char = text_code::check_symbol;
    let _: fn(&str) -> Result<PairingToken, TextCodeError> = text_code::parse;
    assert_eq!(TOKEN_LEN, 20);
}
