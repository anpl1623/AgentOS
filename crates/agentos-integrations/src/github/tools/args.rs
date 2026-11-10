//! The arguments each operation takes, as a model writes them.
//!
//! Every type here is the schema a model is shown and the shape a call is
//! parsed into; none of them is trusted until `validate` has checked it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The most items a list may ask for.
pub const MAX_LIMIT: u8 = 50;

/// What a list asks for when it does not say.
pub const DEFAULT_LIMIT: u8 = 20;

/// Which items a list returns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ListState {
    /// Open only.
    #[default]
    Open,
    /// Closed only.
    Closed,
    /// Both.
    All,
}

impl ListState {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::All => "all",
        }
    }
}

/// The state an issue is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum IssueState {
    /// Reopen.
    Open,
    /// Close.
    Closed,
}

impl IssueState {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }
}

/// How a pull request is merged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MergeMethod {
    /// A merge commit.
    #[default]
    Merge,
    /// One commit, squashed.
    Squash,
    /// The commits replayed onto the base.
    Rebase,
}

impl MergeMethod {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }
}

const fn default_limit() -> u8 {
    DEFAULT_LIMIT
}

/// Arguments for `github.repos.get`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoArgs {
    /// `owner/name`.
    pub repo: String,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Arguments for `github.issues.list`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListIssuesArgs {
    /// `owner/name`.
    pub repo: String,
    /// `open` (the default), `closed` or `all`.
    #[serde(default)]
    pub state: ListState,
    /// Only issues carrying every one of these labels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// How many to return, 1 to 50. Defaults to 20.
    #[serde(default = "default_limit")]
    pub limit: u8,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Arguments for `github.issues.get`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueArgs {
    /// `owner/name`.
    pub repo: String,
    /// The issue number.
    pub number: u64,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Arguments for `github.issues.create`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateIssueArgs {
    /// `owner/name`.
    pub repo: String,
    /// The title.
    pub title: String,
    /// The body, in Markdown.
    #[serde(default)]
    pub body: String,
    /// Labels to apply.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Arguments for `github.issues.comment`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommentArgs {
    /// `owner/name`.
    pub repo: String,
    /// The issue or pull request number.
    pub number: u64,
    /// The comment, in Markdown.
    pub body: String,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Arguments for `github.issues.update`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateIssueArgs {
    /// `owner/name`.
    pub repo: String,
    /// The issue number.
    pub number: u64,
    /// `open` or `closed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<IssueState>,
    /// A new title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A new body, replacing the old one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// The complete new set of labels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Arguments for `github.pulls.list`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListPullsArgs {
    /// `owner/name`.
    pub repo: String,
    /// `open` (the default), `closed` or `all`.
    #[serde(default)]
    pub state: ListState,
    /// How many to return, 1 to 50. Defaults to 20.
    #[serde(default = "default_limit")]
    pub limit: u8,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Arguments for `github.pulls.get`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PullArgs {
    /// `owner/name`.
    pub repo: String,
    /// The pull request number.
    pub number: u64,
    /// Whether to return the diff as well.
    #[serde(default)]
    pub include_diff: bool,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Arguments for `github.pulls.create`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePullArgs {
    /// `owner/name`.
    pub repo: String,
    /// The branch the changes are on, or `user:branch` for a fork.
    pub head: String,
    /// The branch to merge into.
    pub base: String,
    /// The title.
    pub title: String,
    /// The body, in Markdown.
    #[serde(default)]
    pub body: String,
    /// Open it as a draft.
    #[serde(default)]
    pub draft: bool,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Arguments for `github.pulls.comment`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PullCommentArgs {
    /// `owner/name`.
    pub repo: String,
    /// The pull request number.
    pub number: u64,
    /// The comment, in Markdown.
    pub body: String,
    /// A file in the diff, to comment on one of its lines. Give `line` too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The line in `path`, as numbered in the changed file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Arguments for `github.pulls.merge`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeArgs {
    /// `owner/name`.
    pub repo: String,
    /// The pull request number.
    pub number: u64,
    /// The branch the pull request merges into. The merge is refused if the
    /// pull request targets any other.
    pub base: String,
    /// `merge` (the default), `squash` or `rebase`.
    #[serde(default)]
    pub method: MergeMethod,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// Arguments for `github.checks.list`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChecksArgs {
    /// `owner/name`.
    pub repo: String,
    /// A commit SHA, branch or tag.
    pub git_ref: String,
    /// How many to return, 1 to 50. Defaults to 20.
    #[serde(default = "default_limit")]
    pub limit: u8,
    /// The bound account to act as, by label. Needed only when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}
