//! This Mac's name, as a peer sees it on a first run.
//!
//! The *computer name* — the one in System Settings → General → Sharing, and
//! the one AirDrop shows ("Ana's MacBook Air") — rather than the DNS host
//! name (`anas-macbook-air.local`). It is the name a person recognises their
//! own Mac by, which is what a pairing screen on a phone needs.
//!
//! Read with `scutil --get ComputerName`. `scutil` ships with every macOS and
//! is the documented command-line face of the same `SCDynamicStore` value
//! `SCDynamicStoreCopyComputerName` returns; calling it keeps this crate free
//! of a SystemConfiguration binding for one string, read once, at first run.

/// The computer name, or a neutral fallback.
///
/// The fallback is "Mac", not a hostname-shaped guess and not "Pliwee
/// Desktop": a phone showing "Mac" while pairing is telling the truth with
/// less detail, which is the right failure.
pub fn device_name() -> String {
    std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| clean(&s))
        .unwrap_or_else(|| FALLBACK.to_string())
}

const FALLBACK: &str = "Mac";

/// Trims `scutil`'s output to one name, or `None` when there is none.
fn clean(raw: &str) -> Option<String> {
    let name = raw.lines().next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scutil_output_is_trimmed_to_one_line() {
        assert_eq!(
            clean("Ana's MacBook Air\n").as_deref(),
            Some("Ana's MacBook Air")
        );
        assert_eq!(clean("  Studio  \nextra\n").as_deref(), Some("Studio"));
    }

    #[test]
    fn an_empty_answer_is_no_name_rather_than_an_empty_one() {
        assert_eq!(clean(""), None);
        assert_eq!(clean("\n"), None);
        assert_eq!(clean("   \n"), None);
    }

    #[test]
    fn this_mac_has_a_name() {
        // Measured on the machine running the test: `scutil` exists and
        // answers. A fallback here would mean the probe is broken, so it is
        // asserted against, not accepted.
        let name = device_name();
        assert!(!name.is_empty());
        assert_ne!(name, FALLBACK, "scutil --get ComputerName did not answer");
    }
}
