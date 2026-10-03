//! The general pasteboard, for `clipboard.v1`.
//!
//! The macOS implementation of `pliwee-capability-clipboard`'s
//! [`ClipboardBackend`]. Read and write go through `NSPasteboard`, the one
//! public clipboard API on macOS; nothing here shells out to `pbcopy`, which
//! could not mark a clip sensitive.
//!
//! # What it can do
//!
//! | | |
//! | --- | --- |
//! | **read** | `stringForType:` with `NSPasteboardTypeString` on the general pasteboard |
//! | **write** | one `NSPasteboardItem` holding every type, then `clearContents` + `writeObjects:` |
//! | **sensitive** | the clip is also declared as `org.nspasteboard.ConcealedType` — the convention clipboard managers on macOS honour by not keeping it in history. A hint, exactly like `wl-copy --sensitive` and Android's `EXTRA_IS_SENSITIVE`; never enforcement |
//! | **watch** | **unavailable** — see below |
//!
//! There is no PRIMARY selection on macOS, so the "never PRIMARY" rule in the
//! trait is satisfied by construction.
//!
//! # Why there is no watch
//!
//! `NSPasteboard` has no change notification. The documented way to notice a
//! change is to compare `changeCount` against the last value, on a timer —
//! and [`ClipboardBackend::watch_changes`] says, as a rule on implementors,
//! that no implementation may satisfy it by polling. Research doc 10 §6.1
//! recommends amending that rule for platforms with no event source, as
//! PLAT-DEC-009; the decision is open, and an adapter does not get to make it
//! by quietly breaking the contract. Until it is made, this backend reports
//! the watch unavailable, the manager degrades to manual sending, and
//! `pliwee clipboard status` says why — the same honest state a GNOME session
//! was in before the XFIXES watch existed.
//!
//! # Privacy prompts
//!
//! From macOS 15.4 a programmatic read of the general pasteboard can raise a
//! system alert asking the user whether this application may paste. That is
//! the system's decision, made per application in System Settings; this
//! backend does not try to avoid it. A read only ever happens because a
//! person asked to send their clipboard.
//!
//! # Why FFI
//!
//! `NSPasteboard` is an Objective-C class. It is reached through `objc2`'s
//! generated AppKit bindings, which are safe Rust for the calls below except
//! one: reading the `NSPasteboardTypeString` constant, an `extern` static,
//! which [`string_type`] does under a `// SAFETY:` comment.

use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardItem, NSPasteboardType, NSPasteboardTypeString, NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSString};
use pliwee_capability_clipboard::backend::{
    BackendError, BackendResult, ClipboardBackend, ClipboardWatch,
};
use pliwee_capability_clipboard::limits::BACKEND_TIMEOUT;
use pliwee_capability_clipboard::{ClipboardText, TextRejection};

/// The type clipboard managers read as "do not record this".
///
/// From nspasteboard.org, a community convention rather than an Apple API —
/// which is why it is a hint and nothing depends on it.
pub const CONCEALED_TYPE: &str = "org.nspasteboard.ConcealedType";

/// Why there is no automatic sending on macOS. One sentence, shown verbatim
/// by `pliwee clipboard status` and the application.
pub const WATCH_UNAVAILABLE: &str =
    "macOS has no clipboard-change notification, and Pliwee does not poll \
     the clipboard (PLAT-DEC-009 is open). Send the clipboard by hand from \
     Pliwee or with `pliwee clipboard send`; receiving is unaffected";

/// The `NSPasteboard` backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct PasteboardBackend;

impl PasteboardBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ClipboardBackend for PasteboardBackend {
    fn id(&self) -> &'static str {
        "nspasteboard"
    }

    async fn read_text(&self) -> BackendResult<Option<ClipboardText>> {
        let text = bounded(read_general_string).await?;
        match text.map(ClipboardText::validate) {
            None => Ok(None),
            Some(Ok(clip)) => Ok(Some(clip)),
            // Nothing to share, as an empty clipboard is.
            Some(Err(TextRejection::Empty)) => Ok(None),
            Some(Err(why)) => Err(BackendError::Failed(format!(
                "the clipboard holds text that cannot be sent: {why}"
            ))),
        }
    }

    async fn write_text(&self, text: &ClipboardText, sensitive: bool) -> BackendResult<()> {
        let text = text.as_str().to_owned();
        let written = bounded(move || write_general_string(&text, sensitive)).await?;
        if written {
            Ok(())
        } else {
            Err(BackendError::Failed(
                "NSPasteboard refused the write".to_string(),
            ))
        }
    }

    fn watch_changes(&self) -> BackendResult<ClipboardWatch> {
        Err(BackendError::Unavailable(WATCH_UNAVAILABLE.to_string()))
    }

    fn watch_availability(&self) -> Result<(), String> {
        Err(WATCH_UNAVAILABLE.to_string())
    }

    fn describe(&self) -> String {
        "NSPasteboard (general pasteboard); sensitive clips marked \
         org.nspasteboard.ConcealedType; no change notification on macOS"
            .to_string()
    }
}

/// Runs a pasteboard call off the async workers, within the trait's bound.
async fn bounded<T, F>(f: F) -> BackendResult<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    match tokio::time::timeout(BACKEND_TIMEOUT, tokio::task::spawn_blocking(f)).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(join)) => Err(BackendError::Failed(format!(
            "the pasteboard call did not complete: {join}"
        ))),
        Err(_) => Err(BackendError::TimedOut),
    }
}

/// The general pasteboard's plain text, if it holds any.
fn read_general_string() -> Option<String> {
    autoreleasepool(|_| {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard
            .stringForType(string_type())
            .map(|s| s.to_string())
    })
}

/// Replaces the general pasteboard's contents. `true` when AppKit accepted
/// the write.
fn write_general_string(text: &str, sensitive: bool) -> bool {
    autoreleasepool(|_| {
        // One pasteboard item carrying every type, written in one call. A
        // reader therefore sees the text and its concealed marker together or
        // not at all: writing them as two `setString:forType:` calls on the
        // pasteboard would leave a moment in which a clipboard manager could
        // record the text unmarked — the one outcome the marker exists to
        // prevent.
        let item = NSPasteboardItem::new();
        let mut ok = item.setString_forType(&NSString::from_str(text), string_type());
        if sensitive {
            // The marker carries no data; its presence is the signal.
            ok &= item
                .setString_forType(&NSString::from_str(""), &NSString::from_str(CONCEALED_TYPE));
        }
        if !ok {
            return false;
        }
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let writers: Retained<NSArray<ProtocolObject<dyn NSPasteboardWriting>>> =
            NSArray::from_retained_slice(&[ProtocolObject::from_retained(item)]);
        pasteboard.writeObjects(&writers)
    })
}

/// `NSPasteboardTypeString`, the plain-text type.
#[allow(unsafe_code)]
fn string_type() -> &'static NSPasteboardType {
    // SAFETY: `NSPasteboardTypeString` is an immutable `NSString *const`
    // exported by AppKit and initialised before any code in this process
    // runs (AppKit is linked, so the dynamic loader binds it at launch). It
    // is never written, so reading it from any thread is sound, and the
    // object it points to lives for the life of the process.
    unsafe { NSPasteboardTypeString }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_watch_is_reported_unavailable_and_says_why() {
        let b = PasteboardBackend::new();
        let why = b.watch_availability().expect_err("no watch on macOS");
        assert!(why.contains("PLAT-DEC-009"), "{why}");
        assert!(matches!(
            b.watch_changes(),
            Err(BackendError::Unavailable(_))
        ));
    }

    #[test]
    fn the_ordinary_clipboard_and_sensitive_marking_are_both_available() {
        let b = PasteboardBackend::new();
        assert_eq!(b.availability(), Ok(()));
        assert_eq!(b.sensitive_support(), Ok(()));
    }

    #[test]
    fn describe_names_the_api_and_the_missing_watch() {
        let d = PasteboardBackend::new().describe();
        assert!(d.contains("NSPasteboard"), "{d}");
        assert!(d.contains("no change notification"), "{d}");
    }

    // The round trip through the real general pasteboard. Ignored by default
    // because it replaces whatever the person running the tests has copied;
    // run it on purpose:
    //
    //   cargo test -p pliwee-macos -- --ignored the_real_pasteboard
    #[tokio::test]
    #[ignore = "replaces the user's clipboard; run explicitly"]
    async fn the_real_pasteboard_round_trips_plain_and_sensitive_text() {
        let b = PasteboardBackend::new();
        let saved = read_general_string();

        let plain = ClipboardText::validate("pliwee plain round trip").expect("valid");
        b.write_text(&plain, false).await.expect("write plain");
        let back = b.read_text().await.expect("read").expect("some text");
        assert_eq!(back.as_str(), plain.as_str());
        assert!(
            !has_type(CONCEALED_TYPE),
            "a plain clip must not be concealed"
        );

        let secret = ClipboardText::validate("pliwee sensitive round trip").expect("valid");
        b.write_text(&secret, true).await.expect("write sensitive");
        let back = b.read_text().await.expect("read").expect("some text");
        assert_eq!(back.as_str(), secret.as_str());
        assert!(
            has_type(CONCEALED_TYPE),
            "a sensitive clip must be concealed"
        );

        if let Some(text) = saved {
            write_general_string(&text, false);
        }
    }

    fn has_type(name: &str) -> bool {
        autoreleasepool(|_| {
            let wanted = NSString::from_str(name);
            NSPasteboard::generalPasteboard()
                .types()
                .is_some_and(|types| types.to_vec().iter().any(|t| **t == *wanted))
        })
    }
}
