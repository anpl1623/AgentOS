//! Credential values, where tools find them, and their redaction.
//!
//! [`Secret`] holds a value and will not print it; [`CredentialResolver`] is
//! where a tool looks one up by origin and name; and the run's ledger keeps
//! every value released, so that whatever a call hands back can be redacted
//! against them, fragments included.

use std::fmt;

use async_trait::async_trait;

/// The value of a stored credential.
///
/// A secret is the one thing this crate handles that must never be written
/// down, so the type is built to make writing it down awkward: it has no
/// `Display`, no `Serialize`, a `Debug` that prints `[redacted]`, and one
/// accessor, [`Secret::expose`], whose name is what a reviewer greps for. A
/// struct that derives `Debug` and holds one prints nothing of it.
///
/// The bytes are overwritten when the value is dropped. That is best effort and
/// no more: the string the secret was built from, a header value copied out of
/// it and the allocator's own bookkeeping are beyond its reach. What it does
/// guarantee is that no log line, audit record or approval card can render one
/// by accident.
pub struct Secret(Vec<u8>);

impl Secret {
    /// Wrap a credential's value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().into_bytes())
    }

    /// The value itself, for the one place that must send it.
    #[must_use]
    pub fn expose(&self) -> &str {
        // Built only from a `String`, so this never falls back; the fallback
        // is there because a panic here would be worse than an empty header.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    /// Whether the value is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl Clone for Secret {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
        // Keeps the compiler from proving the zeroed buffer is never read and
        // removing the store as dead.
        std::hint::black_box(&self.0);
    }
}

/// Where a tool finds a stored credential, by the origin it is bound to.
///
/// The origin is part of the lookup, not a field beside it. A run that has been
/// hijacked can name any credential it likes against any host it likes, and if
/// the host is not the one the operator stored the credential for, the lookup
/// simply misses. Binding is the control, not the tool's good intentions.
///
/// Tools never hold an implementation directly. The pipeline hands each call one
/// that answers only for the credentials its authorised plan named, records
/// the spend in the audit log before it hands a value over, and keeps every
/// value it releases, so the pipeline knows what to redact and what to report
/// without the tool having to tell it. Asynchronous for that reason: the
/// record is written, not queued, by the time the tool holds the secret.
#[async_trait]
pub trait CredentialResolver: Send + Sync + fmt::Debug {
    /// The credential `name` stored for `origin`, a canonical
    /// `scheme://host[:port]`, if there is one.
    async fn resolve(&self, origin: &str, name: &str) -> Option<Secret>;
}

/// Every credential value released during one run.
///
/// Written by the resolver the pipeline wraps around the run's own, so it is
/// complete whether or not the tool reports what it used; who was given what
/// is the audit chain's to say, and this keeps only the values. It is what
/// output is redacted against: a token a service stored on one call and echoed
/// on a later one is as live as one echoed straight back.
#[derive(Default)]
pub(crate) struct CredentialLedger {
    issued: std::sync::Mutex<Vec<Secret>>,
}

impl CredentialLedger {
    fn entries(&self) -> std::sync::MutexGuard<'_, Vec<Secret>> {
        // A panic while holding this lock leaves a list that is still a list;
        // refusing to redact because of it would be the worse failure.
        self.issued
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Record a released value.
    pub(crate) fn record(&self, secret: Secret) {
        self.entries().push(secret);
    }

    /// How many values have been released.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries().len()
    }

    /// `text` with every released value replaced by [`REDACTED_CREDENTIAL`],
    /// or `None` when it contains none of them. See [`redact_values`] for what
    /// counts as containing one.
    pub(crate) fn redact(&self, text: &str) -> Option<String> {
        let entries = self.entries();
        let values: Vec<&str> = entries.iter().map(Secret::expose).collect();
        redact_values(text, &values)
    }

    /// Whether nothing has been released.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries().is_empty()
    }
}

impl fmt::Debug for CredentialLedger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialLedger")
            .field("issued", &self.entries().len())
            .finish()
    }
}

/// What a released credential's value is replaced with wherever it reappears.
pub const REDACTED_CREDENTIAL: &str = "[redacted credential]";

/// The shortest run of a credential's bytes that is redacted on its own.
///
/// A value is not only echoed whole. A response cut short, a server that
/// quotes the first part of what it was sent, a range request over an error
/// page: each returns a piece, and a piece of a token is most of a token. So
/// any run of at least this many consecutive bytes of a released value is
/// redacted wherever it appears, as far as it continues to match. A value
/// shorter than this is redacted only whole, since a shorter run would match
/// ordinary text. What can still pass is fewer than this many bytes from one
/// place, which is a guess, not a credential.
pub const MIN_REDACTED_FRAGMENT: usize = 8;

/// `text` with every occurrence of each value, and every run of at least
/// [`MIN_REDACTED_FRAGMENT`] bytes of one, replaced by
/// [`REDACTED_CREDENTIAL`]; `None` when there is nothing to replace.
///
/// Overlapping matches become one replacement, so a credential that contains
/// another is not left with part of itself showing, and a match that would
/// split a character takes the whole character.
pub(crate) fn redact_values(text: &str, values: &[&str]) -> Option<String> {
    let mut spans: Vec<(usize, usize)> = values
        .iter()
        .flat_map(|value| exposures(text, value))
        .collect();
    if spans.is_empty() {
        return None;
    }
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let mut redacted = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end) in merged {
        redacted.push_str(&text[at..start]);
        redacted.push_str(REDACTED_CREDENTIAL);
        at = end;
    }
    redacted.push_str(&text[at..]);
    Some(redacted)
}

/// The byte ranges of `text` that show `value` or a long enough run of it,
/// widened to character boundaries.
fn exposures(text: &str, value: &str) -> Vec<(usize, usize)> {
    if value.is_empty() {
        return Vec::new();
    }
    if value.len() < MIN_REDACTED_FRAGMENT {
        return text
            .match_indices(value)
            .map(|(start, found)| (start, start + found.len()))
            .collect();
    }
    let (haystack, needle) = (text.as_bytes(), value.as_bytes());
    let width = MIN_REDACTED_FRAGMENT;
    let mut windows: std::collections::HashMap<&[u8], Vec<usize>> =
        std::collections::HashMap::new();
    for (offset, window) in needle.windows(width).enumerate() {
        windows.entry(window).or_default().push(offset);
    }
    let mut spans = Vec::new();
    let mut at = 0;
    while at + width <= haystack.len() {
        let Some(offsets) = windows.get(&haystack[at..at + width]) else {
            at += 1;
            continue;
        };
        let length = offsets
            .iter()
            .map(|offset| {
                width
                    + haystack[at + width..]
                        .iter()
                        .zip(&needle[offset + width..])
                        .take_while(|(a, b)| a == b)
                        .count()
            })
            .max()
            .unwrap_or(width);
        spans.push((floor_boundary(text, at), ceil_boundary(text, at + length)));
        // Resume inside the tail of this run rather than after it: a value
        // that repeats part of itself can start a second, longer run there.
        at = (at + length + 1).saturating_sub(width).max(at + 1);
    }
    spans
}

fn floor_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_prints_as_redacted_wherever_it_is_held() {
        #[derive(Debug)]
        #[allow(dead_code, reason = "printed, not read")]
        struct Holder {
            secret: Secret,
        }
        let holder = Holder {
            secret: Secret::new("hunter2-value"),
        };
        let printed = format!("{holder:?} {:?}", holder.secret.clone());
        assert!(!printed.contains("hunter2"), "{printed}");
        assert_eq!(holder.secret.expose(), "hunter2-value");
    }

    #[test]
    fn the_ledger_redacts_the_longer_of_two_overlapping_values_whole() {
        let ledger = CredentialLedger::default();
        assert_eq!(ledger.redact("nothing released yet"), None);
        ledger.record(Secret::new("abc"));
        ledger.record(Secret::new("abcdef"));
        ledger.record(Secret::new(""));
        assert_eq!(
            ledger.redact("got abcdef and abc").as_deref(),
            Some("got [redacted credential] and [redacted credential]")
        );
        assert_eq!(ledger.redact("nothing here"), None);
        assert!(!format!("{ledger:?}").contains("abc"));
    }

    #[test]
    fn no_run_of_a_released_value_long_enough_to_matter_survives() {
        const TOKEN: &str = "tok_live_9f8e7d6c5b4a39281706f5e4d3c2b1a0";
        let ledger = CredentialLedger::default();
        ledger.record(Secret::new(TOKEN));

        let k = MIN_REDACTED_FRAGMENT;
        let mut pieces = vec![TOKEN.to_owned()];
        // Every prefix and suffix a cut could leave, and a slice from the
        // middle such as a range request returns.
        for cut in k..TOKEN.len() {
            pieces.push(TOKEN[..cut].to_owned());
            pieces.push(TOKEN[TOKEN.len() - cut..].to_owned());
        }
        pieces.push(TOKEN[10..10 + k].to_owned());
        for piece in pieces {
            let text = format!("received \"Bearer {piece}\" [truncated]");
            let redacted = ledger.redact(&text).unwrap_or_else(|| panic!("{piece}"));
            assert_eq!(
                redacted, "received \"Bearer [redacted credential]\" [truncated]",
                "{piece}"
            );
        }

        // Shorter than the floor is ordinary text, and is left alone.
        assert_eq!(ledger.redact(&format!("id {}", &TOKEN[..k - 1])), None);

        // A token echoed twice, back to back, and one split across a
        // multi-byte character, are both covered whole.
        assert_eq!(
            ledger.redact(&format!("{TOKEN}{TOKEN}")).as_deref(),
            Some("[redacted credential]")
        );
        let ledger = CredentialLedger::default();
        ledger.record(Secret::new("pässwörd-çödé-1"));
        assert_eq!(
            ledger.redact("x ässwörd-çö y").as_deref(),
            Some("x [redacted credential] y")
        );
    }
}
