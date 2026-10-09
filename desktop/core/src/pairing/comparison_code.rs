//! The pairing comparison code: six words both screens show, so a human can
//! check that the two devices see the same pair of identities.
//!
//! ADR-0022 §D5 fixes the digest and the six-word rendering;
//! `docs/architecture/MULTI-DEVICE-MESH-V2.md` §8 fixes the word list:
//!
//! ```text
//! digest  = SHA-256("pliwee/pairing-compare/v1"
//!                   || len32(responder_fp) || responder_fp
//!                   || len32(initiator_fp) || initiator_fp)
//! indices = the first 66 bits of digest, as six consecutive unsigned
//!           11-bit big-endian values
//! shown   = word[i1] " " word[i2] " " ... " " word[i6]
//! ```
//!
//! "First" counts from the most significant bit of digest byte 0, as BIP-39
//! itself reads its 11-bit groups: index 1 is byte 0 and the top three bits
//! of byte 1.
//!
//! `len32` is a big-endian `u32`, as in the pairing proof. The responder is
//! always the issuer and TLS server, the initiator always the joiner and TLS
//! client (ADR-0022 §D5), so the order of the two fingerprints is tied to a
//! role and never to which device does the computing. [`RoleFingerprints`]
//! names both fields so a caller cannot swap them by position.
//!
//! This module computes and renders, and nothing else. It opens no
//! connection, prompts nobody, and creates no trust: a matching code is
//! something a human confirms, never an authorization input here.
//!
//! The code is derived from two public fingerprints and is not a secret.
//!
//! # The word list
//!
//! `bip39-english.txt` is the canonical English BIP-39 word list, vendored
//! byte for byte from `bip-0039/english.txt` in the `bitcoin/bips`
//! repository (<https://github.com/bitcoin/bips/blob/master/bip-0039/english.txt>):
//! 2048 lowercase ASCII words in their published order, one per line, LF
//! line endings. It is reproduced unmodified because MULTI-DEVICE-MESH-V2 §8
//! requires exactly that list, and it is never downloaded at runtime.
//!
//! License: the BIP-0039 document declares no license of its own, and the
//! list is the published data of an open standard that wallet software
//! reproduces verbatim. This note records that provenance; it does not
//! claim terms the upstream document does not state.
//!
//! Its SHA-256 is pinned by a test against the published value, and its
//! shape (2048 non-empty lowercase words, strictly ascending, so distinct) is
//! checked when this crate compiles. A sibling `.gitattributes` disables
//! end-of-line conversion, so no checkout can alter it. Nothing here sorts,
//! case-folds or formats by locale: the order is the file's, and the words
//! are emitted as they are stored.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::fingerprint::Fingerprint;

/// Domain separator of the comparison digest, distinct from the proof's and
/// the confirmation's.
pub const DOMAIN: &[u8] = b"pliwee/pairing-compare/v1";

/// Words in a comparison code.
pub const WORDS: usize = 6;

/// Digest bits each word carries. `WORDS * BITS_PER_WORD` = 66.
pub const BITS_PER_WORD: u32 = 11;

/// Entries in the word list: one per 11-bit value.
pub const LIST_LEN: usize = 1 << BITS_PER_WORD;

/// The vendored list, exactly as published.
const LIST: &str = include_str!("bip39-english.txt");

const _: () = assert!(list_is_well_formed(LIST.as_bytes()));

/// Byte offset of each word in [`LIST`], plus one past the end of the last
/// line, so word `i` is `LIST[STARTS[i]..STARTS[i + 1] - 1]`.
static STARTS: [usize; LIST_LEN + 1] = word_starts(LIST.as_bytes());

/// The two pairing fingerprints, named by role.
///
/// `responder` is the issuer's (TLS server, pairing-proof responder);
/// `initiator` is the joiner's (TLS client, pairing-proof initiator). Both
/// peers fill it the same way round, whichever of them is computing.
#[derive(Debug, Clone, Copy)]
pub struct RoleFingerprints<'a> {
    pub responder: &'a Fingerprint,
    pub initiator: &'a Fingerprint,
}

/// A comparison code: six indices into the word list.
///
/// `Display` renders the six words, lowercase ASCII, separated by one ASCII
/// space, with nothing before or after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComparisonCode {
    indices: [u16; WORDS],
}

impl ComparisonCode {
    /// The comparison code of a pairing between `fingerprints.responder` and
    /// `fingerprints.initiator`.
    pub fn new(fingerprints: RoleFingerprints<'_>) -> Self {
        Self::from_digest(&digest(fingerprints))
    }

    /// Reads six 11-bit indices from the first 66 bits of `digest`, most
    /// significant bit first. The remaining 190 bits are not used.
    fn from_digest(digest: &[u8; 32]) -> Self {
        let mut head = [0u8; 16];
        head.copy_from_slice(&digest[..16]);
        let bits = u128::from_be_bytes(head);

        let mut indices = [0u16; WORDS];
        for (i, slot) in (0u32..).zip(indices.iter_mut()) {
            let shift = u128::BITS - BITS_PER_WORD * (i + 1);
            *slot = ((bits >> shift) & (LIST_LEN as u128 - 1)) as u16;
        }
        Self { indices }
    }

    /// The six word-list indices, each below [`LIST_LEN`].
    pub fn indices(&self) -> [u16; WORDS] {
        self.indices
    }

    /// The six words, in order.
    pub fn words(&self) -> [&'static str; WORDS] {
        self.indices.map(word_at)
    }
}

impl fmt::Display for ComparisonCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, word) in self.words().into_iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            f.write_str(word)?;
        }
        Ok(())
    }
}

/// The comparison digest of ADR-0022 §D5.
pub fn digest(fingerprints: RoleFingerprints<'_>) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DOMAIN);
    update_len_prefixed(&mut h, fingerprints.responder.as_bytes());
    update_len_prefixed(&mut h, fingerprints.initiator.as_bytes());
    h.finalize().into()
}

fn update_len_prefixed(h: &mut Sha256, data: &[u8]) {
    h.update((data.len() as u32).to_be_bytes());
    h.update(data);
}

/// The word at `index` in the list, or `None` past its end.
pub fn word(index: u16) -> Option<&'static str> {
    (usize::from(index) < LIST_LEN).then(|| word_at(index))
}

/// Callers pass an index below [`LIST_LEN`]: an 11-bit value, or one checked
/// by [`word`].
fn word_at(index: u16) -> &'static str {
    let i = usize::from(index);
    &LIST[STARTS[i]..STARTS[i + 1] - 1]
}

/// `true` when `list` is exactly [`LIST_LEN`] lines, each a non-empty run of
/// `a`-`z` ended by `\n`, in strictly ascending byte order. Evaluated at
/// compile time: a truncated, re-encoded or CRLF copy of the list does not
/// build.
const fn list_is_well_formed(list: &[u8]) -> bool {
    let mut count = 0;
    let mut start = 0;
    let mut prev_start = 0;
    let mut prev_len = 0;
    let mut i = 0;
    while i < list.len() {
        let byte = list[i];
        if byte == b'\n' {
            let len = i - start;
            if len == 0 {
                return false;
            }
            if count > 0 && !precedes(list, prev_start, prev_len, start, len) {
                return false;
            }
            prev_start = start;
            prev_len = len;
            count += 1;
            start = i + 1;
        } else if !byte.is_ascii_lowercase() {
            return false;
        }
        i += 1;
    }
    start == list.len() && count == LIST_LEN
}

/// Strict byte-wise lexicographic order of two words inside `list`.
const fn precedes(list: &[u8], a: usize, a_len: usize, b: usize, b_len: usize) -> bool {
    let mut i = 0;
    while i < a_len && i < b_len {
        if list[a + i] != list[b + i] {
            return list[a + i] < list[b + i];
        }
        i += 1;
    }
    a_len < b_len
}

const fn word_starts(list: &[u8]) -> [usize; LIST_LEN + 1] {
    let mut starts = [0; LIST_LEN + 1];
    let mut n = 1;
    let mut i = 0;
    while i < list.len() && n <= LIST_LEN {
        if list[i] == b'\n' {
            starts[n] = i + 1;
            n += 1;
        }
        i += 1;
    }
    starts
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// SHA-256 of `bip-0039/english.txt` as published (the value
    /// python-mnemonic and Trezor check), and of the 2048 words concatenated
    /// with no separator (the value bitcoinj checks). Two independent pins.
    const LIST_FILE_SHA256: &str =
        "2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda";
    const LIST_WORDS_SHA256: &str =
        "ad90bf3beb7b0eb7e5acd74727dc0da96e0a280a258354e7293fb7e211ac03db";

    fn fp(bytes: [u8; 32]) -> Fingerprint {
        Fingerprint::from_hex(&data_encoding::HEXLOWER.encode(&bytes)).expect("32-byte hex")
    }

    fn counting(from: u8) -> [u8; 32] {
        std::array::from_fn(|i| from + i as u8)
    }

    fn code(responder: &Fingerprint, initiator: &Fingerprint) -> ComparisonCode {
        ComparisonCode::new(RoleFingerprints {
            responder,
            initiator,
        })
    }

    fn hex(bytes: &[u8]) -> String {
        data_encoding::HEXLOWER.encode(bytes)
    }

    fn all_words() -> Vec<&'static str> {
        (0..LIST_LEN as u16).map(word_at).collect()
    }

    // ---------------------------------------------------------------- list --

    #[test]
    fn vendored_list_is_byte_identical_to_the_published_file() {
        assert_eq!(hex(&Sha256::digest(LIST.as_bytes())), LIST_FILE_SHA256);
        let joined: String = all_words().concat();
        assert_eq!(hex(&Sha256::digest(joined.as_bytes())), LIST_WORDS_SHA256);
    }

    #[test]
    fn list_has_exactly_2048_distinct_canonical_entries() {
        let words = all_words();
        assert_eq!(words.len(), 2048);
        assert_eq!(words.iter().collect::<HashSet<_>>().len(), 2048);
        assert_eq!(LIST.lines().count(), 2048);
        assert_eq!(LIST.lines().collect::<Vec<_>>(), words);
        for w in &words {
            assert!((3..=8).contains(&w.len()), "{w:?}");
            assert!(w.bytes().all(|b| b.is_ascii_lowercase()), "{w:?}");
        }
        // BIP-39 English: the first four letters identify a word uniquely.
        let prefixes: HashSet<_> = words.iter().map(|w| &w[..w.len().min(4)]).collect();
        assert_eq!(prefixes.len(), 2048);
        assert_eq!(words.first(), Some(&"abandon"));
        assert_eq!(words.last(), Some(&"zoo"));
    }

    #[test]
    fn word_is_bounded_by_the_list() {
        assert_eq!(word(0), Some("abandon"));
        assert_eq!(word(2047), Some("zoo"));
        assert_eq!(word(2048), None);
        assert_eq!(word(u16::MAX), None);
    }

    #[test]
    fn well_formedness_check_rejects_damaged_lists() {
        assert!(list_is_well_formed(LIST.as_bytes()));
        let crlf = LIST.replace('\n', "\r\n");
        assert!(!list_is_well_formed(crlf.as_bytes()));
        let upper = LIST.replacen("abandon", "Abandon", 1);
        assert!(!list_is_well_formed(upper.as_bytes()));
        let truncated = &LIST[..LIST.len() - "zoo\n".len()];
        assert!(!list_is_well_formed(truncated.as_bytes()));
        let no_final_newline = &LIST[..LIST.len() - 1];
        assert!(!list_is_well_formed(no_final_newline.as_bytes()));
        let duplicated = LIST.replacen("ability\n", "abandon\n", 1);
        assert!(!list_is_well_formed(duplicated.as_bytes()));
        let swapped = LIST.replacen("abandon\nability\n", "ability\nabandon\n", 1);
        assert!(!list_is_well_formed(swapped.as_bytes()));
    }

    // -------------------------------------------------------------- vectors --

    /// Computed independently of this module: the message assembled with
    /// `printf` and hashed with coreutils `sha256sum`, the indices read with
    /// shell arithmetic, the words with `sed -n` on the vendored list.
    struct Vector {
        responder: [u8; 32],
        initiator: [u8; 32],
        digest: &'static str,
        indices: [u16; WORDS],
        shown: &'static str,
    }

    fn vectors() -> [Vector; 3] {
        [
            Vector {
                responder: counting(0x00),
                initiator: counting(0x20),
                digest: "3e07e4471da25594c5ea7d109deca9327551ea44f2a43b9f13fb8db7e9c464b5",
                indices: [496, 505, 142, 474, 298, 1619],
                shown: "dignity dish balcony deputy census skill",
            },
            Vector {
                responder: counting(0x20),
                initiator: counting(0x00),
                digest: "51393155a0a1dc5f2fc5535c21b4e3580f5575926f77998a9d372105387c76ae",
                indices: [649, 1612, 683, 522, 238, 380],
                shown: "eye sister fever donor build convince",
            },
            Vector {
                responder: [0x00; 32],
                initiator: [0xff; 32],
                digest: "1b29fa926a4ef15e5347e0f3d6bc9016aff7171a242e2f974bc9860edfdaca7e",
                indices: [217, 638, 1316, 1700, 1912, 1401],
                shown: "brass exist pig stand upper quality",
            },
        ]
    }

    #[test]
    fn known_fingerprints_produce_the_known_code() {
        for v in vectors() {
            let (r, i) = (fp(v.responder), fp(v.initiator));
            let roles = RoleFingerprints {
                responder: &r,
                initiator: &i,
            };
            assert_eq!(hex(&digest(roles)), v.digest);
            let c = ComparisonCode::new(roles);
            assert_eq!(c.indices(), v.indices);
            assert_eq!(c.to_string(), v.shown);
            assert_eq!(c.words().join(" "), v.shown);
        }
    }

    #[test]
    fn digest_is_the_specified_length_prefixed_message() {
        let (r, i) = (fp(counting(0x00)), fp(counting(0x20)));
        let mut msg = Vec::new();
        msg.extend_from_slice(b"pliwee/pairing-compare/v1");
        msg.extend_from_slice(&[0, 0, 0, 32]);
        msg.extend_from_slice(r.as_bytes());
        msg.extend_from_slice(&[0, 0, 0, 32]);
        msg.extend_from_slice(i.as_bytes());
        assert_eq!(msg.len(), 97);
        let roles = RoleFingerprints {
            responder: &r,
            initiator: &i,
        };
        assert_eq!(digest(roles), <[u8; 32]>::from(Sha256::digest(&msg)));
    }

    #[test]
    fn reversing_the_roles_gives_the_reversed_vector() {
        let (a, b) = (fp(counting(0x00)), fp(counting(0x20)));
        let forward = code(&a, &b);
        let reversed = code(&b, &a);
        assert_ne!(forward, reversed);
        assert_eq!(forward.to_string(), vectors()[0].shown);
        assert_eq!(reversed.to_string(), vectors()[1].shown);
    }

    // ---------------------------------------------------------------- bits --

    fn digest_with_bits(set: impl Fn(usize) -> bool) -> [u8; 32] {
        let mut d = [0u8; 32];
        for bit in (0..256).filter(|&b| set(b)) {
            d[bit / 8] |= 0x80 >> (bit % 8);
        }
        d
    }

    #[test]
    fn indices_come_from_exactly_the_first_66_bits() {
        let first_66 = ComparisonCode::from_digest(&digest_with_bits(|b| b < 66));
        assert_eq!(first_66.indices(), [2047; WORDS]);

        let all_but_first_66 = ComparisonCode::from_digest(&digest_with_bits(|b| b >= 66));
        assert_eq!(all_but_first_66.indices(), [0; WORDS]);
        assert_eq!(
            all_but_first_66.to_string(),
            "abandon abandon abandon abandon abandon abandon"
        );
    }

    #[test]
    fn each_of_the_first_66_bits_lands_in_one_index_msb_first() {
        for bit in 0..66 {
            let c = ComparisonCode::from_digest(&digest_with_bits(|b| b == bit));
            let mut expected = [0u16; WORDS];
            expected[bit / 11] = 1 << (10 - bit % 11);
            assert_eq!(c.indices(), expected, "bit {bit}");
        }
        for bit in 66..256 {
            let c = ComparisonCode::from_digest(&digest_with_bits(|b| b == bit));
            assert_eq!(c.indices(), [0; WORDS], "bit {bit}");
        }
    }

    // -------------------------------------------------------------- render --

    #[test]
    fn output_is_six_lowercase_words_separated_by_single_spaces() {
        for n in 0u8..=255 {
            let r = fp(Sha256::digest([b'r', n]).into());
            let i = fp(Sha256::digest([b'i', n]).into());
            let shown = code(&r, &i).to_string();

            assert!(
                shown.bytes().all(|b| b == b' ' || b.is_ascii_lowercase()),
                "{shown:?}"
            );
            assert!(
                !shown.starts_with(' ') && !shown.ends_with(' '),
                "{shown:?}"
            );
            assert!(!shown.contains("  "), "{shown:?}");
            let words: Vec<_> = shown.split(' ').collect();
            assert_eq!(words.len(), WORDS, "{shown:?}");
            for w in words {
                assert!(LIST.lines().any(|l| l == w), "{w:?}");
            }
        }
    }

    #[test]
    fn words_are_the_list_lines_at_the_indices() {
        // Read through `str::lines`, not through the offset table.
        let lines: Vec<_> = LIST.lines().collect();
        let c = code(&fp([0x01; 32]), &fp([0x02; 32]));
        for (w, i) in c.words().iter().zip(c.indices()) {
            assert_eq!(*w, lines[usize::from(i)]);
            assert_eq!(word(i), Some(*w));
        }
    }
}
