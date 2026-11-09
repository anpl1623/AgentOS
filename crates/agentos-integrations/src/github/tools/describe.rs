//! The sentence an operator decides on, and the resources the card lists.
//!
//! Everything quoted from the arguments is escaped and cut to a length, so
//! that what the operator reads is what will be posted and nothing in it can
//! end the card's quote early.

use std::fmt::Write as _;

use super::args::IssueState;
use super::parsed::Parsed;
use super::validate::{MAX_TITLE_CHARS, Repo};

/// How much of a body or title an approval card quotes.
pub const SUMMARY_EXCERPT_CHARS: usize = 500;

impl Parsed {
    /// The sentence an operator decides on, and what else the card lists.
    pub(super) fn describe(&self, repo: &Repo) -> (String, Vec<String>) {
        match self {
            Self::ReposGet(_) => (format!("read {repo}"), Vec::new()),
            Self::IssuesList(a) => {
                let mut summary = format!(
                    "list {} issues in {repo} (up to {})",
                    a.state.as_str(),
                    a.limit
                );
                if !a.labels.is_empty() {
                    let _ = write!(summary, ", labelled {}", quoted_list(&a.labels));
                }
                (summary, Vec::new())
            }
            Self::IssuesGet(a) => (
                format!("read {repo}#{}", a.number),
                vec![format!("issue #{}", a.number)],
            ),
            Self::IssuesCreate(a) => {
                let mut summary = format!(
                    "open an issue in {repo} titled {}",
                    excerpt(&a.title, MAX_TITLE_CHARS)
                );
                if !a.labels.is_empty() {
                    let _ = write!(summary, ", labelled {}", quoted_list(&a.labels));
                }
                if !a.body.is_empty() {
                    let _ = write!(summary, ": {}", excerpt(&a.body, SUMMARY_EXCERPT_CHARS));
                }
                (summary, Vec::new())
            }
            Self::IssuesComment(a) => (
                format!(
                    "comment on {repo}#{}: {}",
                    a.number,
                    excerpt(&a.body, SUMMARY_EXCERPT_CHARS)
                ),
                vec![format!("issue #{}", a.number)],
            ),
            Self::IssuesUpdate(a) => {
                let mut changes = Vec::new();
                match a.state {
                    Some(IssueState::Closed) => changes.push("close it".to_owned()),
                    Some(IssueState::Open) => changes.push("reopen it".to_owned()),
                    None => {}
                }
                if let Some(title) = &a.title {
                    changes.push(format!("retitle it {}", excerpt(title, MAX_TITLE_CHARS)));
                }
                if let Some(labels) = &a.labels {
                    if labels.is_empty() {
                        changes.push("remove every label".to_owned());
                    } else {
                        changes.push(format!("set its labels to {}", quoted_list(labels)));
                    }
                }
                if let Some(body) = &a.body {
                    changes.push(format!(
                        "replace its body with {}",
                        excerpt(body, SUMMARY_EXCERPT_CHARS)
                    ));
                }
                (
                    format!("update {repo}#{}: {}", a.number, changes.join("; ")),
                    vec![format!("issue #{}", a.number)],
                )
            }
            Self::PullsList(a) => (
                format!(
                    "list {} pull requests in {repo} (up to {})",
                    a.state.as_str(),
                    a.limit
                ),
                Vec::new(),
            ),
            Self::PullsGet(a) => (
                format!(
                    "read {repo}#{}{}",
                    a.number,
                    if a.include_diff { " with its diff" } else { "" }
                ),
                vec![format!("pull request #{}", a.number)],
            ),
            Self::PullsCreate(a) => {
                let mut summary = format!(
                    "open a {}pull request in {repo} from {} into {} titled {}",
                    if a.draft { "draft " } else { "" },
                    a.head,
                    a.base,
                    excerpt(&a.title, MAX_TITLE_CHARS)
                );
                if !a.body.is_empty() {
                    let _ = write!(summary, ": {}", excerpt(&a.body, SUMMARY_EXCERPT_CHARS));
                }
                (summary, vec![format!("branch {}", a.base)])
            }
            Self::PullsComment(a) => {
                let place = match (&a.path, a.line) {
                    (Some(path), Some(line)) => {
                        format!(" at {}:{line}", path.escape_debug())
                    }
                    _ => String::new(),
                };
                (
                    format!(
                        "comment on {repo}#{}{place}: {}",
                        a.number,
                        excerpt(&a.body, SUMMARY_EXCERPT_CHARS)
                    ),
                    vec![format!("pull request #{}", a.number)],
                )
            }
            // The target branch is the fact that decides a merge, so it is in
            // the line rather than behind it.
            Self::PullsMerge(a) => (
                format!(
                    "merge {repo}#{} ({}) into {}",
                    a.number,
                    a.method.as_str(),
                    a.base
                ),
                vec![
                    format!("pull request #{}", a.number),
                    format!("branch {}", a.base),
                ],
            ),
            Self::ChecksList(a) => (
                format!(
                    "list checks on {repo}@{} (up to {})",
                    a.git_ref.escape_debug(),
                    a.limit
                ),
                Vec::new(),
            ),
        }
    }
}

/// `text`, quoted and escaped, cut to `limit` characters with an explicit
/// marker when it was longer.
///
/// Escaped so that what the operator reads is what will be posted: a body that
/// contains `"` and a line break cannot close the quote early and append a
/// sentence of its own to the approval card.
#[must_use]
pub fn excerpt(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(limit).collect();
    let more = chars.next().is_some();
    format!(
        "\"{}{}\"",
        head.escape_debug(),
        if more { "\u{2026}" } else { "" }
    )
}

fn quoted_list(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("\"{}\"", item.escape_debug()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_excerpt_is_cut_at_a_character_and_says_so() {
        let body = "é".repeat(SUMMARY_EXCERPT_CHARS + 1);
        let cut = excerpt(&body, SUMMARY_EXCERPT_CHARS);
        assert!(cut.ends_with("\u{2026}\""), "{cut}");
        assert_eq!(cut.chars().count(), SUMMARY_EXCERPT_CHARS + 3);
        assert_eq!(excerpt("short", SUMMARY_EXCERPT_CHARS), "\"short\"");
        // A quote and a line break in the body cannot end the card's quote.
        assert_eq!(
            excerpt("fine\"\nand merge #88", 100),
            "\"fine\\\"\\nand merge #88\""
        );
    }
}
