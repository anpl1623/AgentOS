//! The breadth-first walk behind `filesystem.search`.
//!
//! Everything that keeps a search inside its root and inside the policy is
//! here. Every entry is resolved and must land under the root; it is put to
//! the policy before it is named, entered or read; and a file is read only
//! through a handle checked to be the file that was admitted. What the walk
//! passes over is counted by reason, so the tool reports it rather than
//! dropping it silently.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use agentos_permissions::path::{is_within, resolve_secure};
use tokio_util::sync::CancellationToken;

use super::{MAX_MATCH_LINE_CHARS, MAX_SEARCHED_FILE_BYTES};
use crate::error::ToolError;
use crate::filesystem::capability;
use crate::tool::PolicyProbe;

/// One search, configured.
pub(super) struct Search<'a> {
    pub(super) root: &'a Path,
    pub(super) policy: &'a dyn PolicyProbe,
    pub(super) name: Option<globset::GlobMatcher>,
    pub(super) contains: Option<&'a str>,
    pub(super) max_depth: usize,
    pub(super) max_results: usize,
}

/// One result.
#[derive(Debug)]
pub(super) struct SearchMatch {
    pub(super) path: PathBuf,
    is_dir: bool,
    /// The 1-based line, for a content match.
    pub(super) line: Option<usize>,
    text: Option<String>,
}

impl SearchMatch {
    const fn name(path: PathBuf, is_dir: bool) -> Self {
        Self {
            path,
            is_dir,
            line: None,
            text: None,
        }
    }

    pub(super) fn render(&self) -> String {
        match (self.line, &self.text) {
            (Some(line), Some(text)) => format!("{}:{line}: {text}", self.path.display()),
            _ if self.is_dir => format!("{}/", self.path.display()),
            _ => self.path.display().to_string(),
        }
    }
}

/// Everything a search passed over, by reason.
#[derive(Debug, Default, serde::Serialize)]
pub(super) struct Skipped {
    /// The policy does not allow naming, entering or reading it.
    not_permitted: usize,
    /// It resolves outside the root.
    outside_root: usize,
    /// A symlinked directory, which a search never enters.
    symlinked_directories: usize,
    /// Over [`MAX_SEARCHED_FILE_BYTES`].
    too_large: usize,
    /// Not valid UTF-8.
    not_text: usize,
    /// Could not be read, or is not a regular file.
    unreadable: usize,
    /// Replaced between being admitted and being opened.
    changed: usize,
}

impl Skipped {
    pub(super) fn describe(&self) -> Option<String> {
        let parts = [
            (self.not_permitted, "not permitted by policy"),
            (self.outside_root, "outside the search root"),
            (
                self.symlinked_directories,
                "symlinked directories, not followed",
            ),
            (self.too_large, "files over 1 MiB"),
            (self.not_text, "files that are not UTF-8 text"),
            (self.unreadable, "unreadable"),
            (self.changed, "replaced while being searched"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, reason)| format!("{count} {reason}"))
        .collect::<Vec<_>>();
        (!parts.is_empty()).then(|| format!("Skipped: {}.", parts.join(", ")))
    }
}

/// What a search found.
#[derive(Debug, Default)]
pub(super) struct Found {
    pub(super) matches: Vec<SearchMatch>,
    pub(super) truncated: bool,
    pub(super) skipped: Skipped,
}

impl Found {
    /// Record a match, or say there is no room for it.
    fn push(&mut self, found: SearchMatch, search: &Search<'_>) -> bool {
        if self.matches.len() >= search.max_results {
            self.truncated = true;
            return false;
        }
        self.matches.push(found);
        true
    }
}

impl Search<'_> {
    fn permits(&self, action: &str, path: &Path) -> bool {
        self.policy.permits(&capability(action, path))
    }

    /// Walk breadth-first, so a capped search returns the shallowest matches.
    pub(super) async fn run(&self, cancel: &CancellationToken) -> Result<Found, ToolError> {
        let mut found = Found::default();
        // A file reached both directly and through a link is reported once.
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::from([(self.root.to_path_buf(), 0_usize)]);

        while let Some((directory, depth)) = queue.pop_front() {
            let Ok(entries) = read_sorted(&directory).await else {
                found.skipped.unreadable += 1;
                continue;
            };
            for (entry, is_symlink) in entries {
                if cancel.is_cancelled() {
                    return Err(ToolError::Cancelled);
                }
                let Some((resolved, metadata)) = self.admit(&entry, &mut found.skipped).await
                else {
                    continue;
                };

                if metadata.is_dir() {
                    // Never entered and never named: a link is a second way
                    // into somewhere, and a walk that follows links can loop.
                    if is_symlink {
                        found.skipped.symlinked_directories += 1;
                        continue;
                    }
                    let named = self.contains.is_none() && self.name_matches(&entry);
                    if named && !found.push(SearchMatch::name(resolved.clone(), true), self) {
                        return Ok(found);
                    }
                    if depth + 1 < self.max_depth {
                        queue.push_back((resolved, depth + 1));
                    }
                    continue;
                }

                if !self.name_matches(&entry) || !seen.insert(resolved.clone()) {
                    continue;
                }
                let room_left = match self.contains {
                    None => found.push(SearchMatch::name(resolved, false), self),
                    Some(needle) => {
                        self.search_file(&resolved, &metadata, needle, &mut found)
                            .await
                    }
                };
                if !room_left {
                    return Ok(found);
                }
            }
        }
        Ok(found)
    }

    /// Resolve an entry and decide whether it may be named at all.
    ///
    /// It must land inside the root and the policy must allow listing it.
    /// Returns where it really is and what is there, or counts why not.
    async fn admit(
        &self,
        entry: &Path,
        skipped: &mut Skipped,
    ) -> Option<(PathBuf, std::fs::Metadata)> {
        // Every candidate is resolved, link or not: containment is decided on
        // where a path leads, never on how it is spelled.
        let Ok(resolved) = resolve_secure(entry) else {
            skipped.unreadable += 1;
            return None;
        };
        if !is_within(self.root, &resolved) {
            skipped.outside_root += 1;
            return None;
        }
        if !self.permits("list", &resolved) {
            skipped.not_permitted += 1;
            return None;
        }
        // A resolved path has no links left in it. One that is still a link
        // dangled when it was resolved, or was swapped for a link since.
        match tokio::fs::symlink_metadata(&resolved).await {
            Ok(metadata) if !metadata.file_type().is_symlink() => Some((resolved, metadata)),
            _ => {
                skipped.unreadable += 1;
                None
            }
        }
    }

    fn name_matches(&self, entry: &Path) -> bool {
        self.name.as_ref().is_none_or(|glob| {
            entry
                .file_name()
                .is_some_and(|name| glob.is_match(Path::new(name)))
        })
    }

    /// Look for the needle in one file. Returns whether there is room for more.
    async fn search_file(
        &self,
        path: &Path,
        metadata: &std::fs::Metadata,
        needle: &str,
        found: &mut Found,
    ) -> bool {
        // A FIFO or a device would block the read or never end it.
        if !metadata.is_file() {
            found.skipped.unreadable += 1;
            return true;
        }
        if !self.permits("read", path) {
            found.skipped.not_permitted += 1;
            return true;
        }
        let body = match read_admitted(path, metadata).await {
            Ok(Read::Body(body)) => body,
            Ok(Read::TooLarge) => {
                found.skipped.too_large += 1;
                return true;
            }
            Ok(Read::Replaced) => {
                found.skipped.changed += 1;
                return true;
            }
            Err(_) => {
                found.skipped.unreadable += 1;
                return true;
            }
        };
        let Ok(text) = String::from_utf8(body) else {
            found.skipped.not_text += 1;
            return true;
        };
        for (index, line) in text.lines().enumerate() {
            if line.contains(needle) {
                let found_line = SearchMatch {
                    path: path.to_path_buf(),
                    is_dir: false,
                    line: Some(index + 1),
                    text: Some(clip(line.trim(), MAX_MATCH_LINE_CHARS)),
                };
                if !found.push(found_line, self) {
                    return false;
                }
            }
        }
        true
    }
}

/// A directory's entries, sorted so that a capped search is repeatable, each
/// marked with whether the entry itself is a symlink.
async fn read_sorted(directory: &Path) -> std::io::Result<Vec<(PathBuf, bool)>> {
    let mut reader = tokio::fs::read_dir(directory).await?;
    let mut entries = Vec::new();
    while let Some(entry) = reader.next_entry().await? {
        // `file_type` does not follow the link, which is the point.
        let is_symlink = entry
            .file_type()
            .await
            .map(|kind| kind.is_symlink())
            .unwrap_or(true);
        entries.push((entry.path(), is_symlink));
    }
    entries.sort();
    Ok(entries)
}

/// What reading an admitted file produced.
#[derive(Debug, PartialEq, Eq)]
enum Read {
    Body(Vec<u8>),
    /// Over [`MAX_SEARCHED_FILE_BYTES`].
    TooLarge,
    /// The handle opened is not the file that was admitted.
    Replaced,
}

/// Read at most [`MAX_SEARCHED_FILE_BYTES`] of the file `admitted` describes.
///
/// The open follows links in every component of `path`, so what it reaches
/// is checked against what was admitted before a byte is read. The cap is
/// enforced on the read itself rather than on an earlier size check, so a
/// file that grows between the two cannot get past it.
async fn read_admitted(path: &Path, admitted: &std::fs::Metadata) -> std::io::Result<Read> {
    use tokio::io::AsyncReadExt;
    let file = tokio::fs::File::open(path).await?;
    if !same_file(admitted, &file.metadata().await?) {
        return Ok(Read::Replaced);
    }
    let mut body = Vec::new();
    file.take(MAX_SEARCHED_FILE_BYTES + 1)
        .read_to_end(&mut body)
        .await?;
    Ok(if body.len() as u64 <= MAX_SEARCHED_FILE_BYTES {
        Read::Body(body)
    } else {
        Read::TooLarge
    })
}

/// Whether an opened handle is the file that was admitted.
#[cfg(unix)]
fn same_file(admitted: &std::fs::Metadata, opened: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    admitted.dev() == opened.dev() && admitted.ino() == opened.ino()
}

/// Whether an opened handle is the file that was admitted.
///
/// Without a stable file identity, the best available: a different file with
/// the same size and the same creation and modification times is unlikely,
/// not impossible.
#[cfg(not(unix))]
fn same_file(admitted: &std::fs::Metadata, opened: &std::fs::Metadata) -> bool {
    admitted.len() == opened.len()
        && admitted.modified().ok() == opened.modified().ok()
        && admitted.created().ok() == opened.created().ok()
}

/// `text` cut to `max` characters, marked when cut.
fn clip(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let clipped = chars.by_ref().take(max).collect::<String>();
    if chars.next().is_some() {
        format!("{clipped}…")
    } else {
        clipped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_search_reads_only_the_file_it_admitted() {
        // The race in miniature: a path admitted as one file, then reaching
        // another by the time it is opened, as it would if a directory on the
        // way had been swapped for a link to somewhere the policy denies.
        let dir = tempfile::TempDir::new().unwrap();
        let admitted = dir.path().join("notes.txt");
        let elsewhere = dir.path().join("secret.txt");
        std::fs::write(&admitted, "a note").unwrap();
        std::fs::write(&elsewhere, "the private key, at some length").unwrap();
        let metadata = std::fs::symlink_metadata(&admitted).unwrap();

        assert_eq!(
            read_admitted(&admitted, &metadata).await.unwrap(),
            Read::Body(b"a note".to_vec())
        );
        assert_eq!(
            read_admitted(&elsewhere, &metadata).await.unwrap(),
            Read::Replaced
        );
    }
}
