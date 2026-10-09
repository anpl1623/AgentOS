//! What the GitHub tools keep of a response.
//!
//! A raw GitHub issue is sixty-odd fields, most of them URLs a model will try
//! to follow, and the half-dozen that matter are the ones below. Each type here
//! deserialises straight from the API payload through a private wire type and
//! drops everything it does not name, so nothing passes through by accident
//! when GitHub adds a field. The same projection is rendered as text for the
//! model and attached as structured data for the UI; the two cannot disagree
//! because one is printed from the other.
//!
//! Every string here was written by somebody outside the runtime. A title or a
//! body is a text field a stranger can type into, and the rendering only lays
//! it out; the pipeline is what labels it untrusted.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// The login shown for an author GitHub no longer has, as its own UI does.
const GHOST: &str = "ghost";

mod wire {
    //! The payload shapes, as far as the projections read them.

    use serde::Deserialize;

    #[derive(Deserialize)]
    pub(super) struct User {
        pub(super) login: String,
    }

    /// A label is an object in most responses and a bare name in some.
    #[derive(Deserialize)]
    #[serde(untagged)]
    pub(super) enum Label {
        Object { name: String },
        Name(String),
    }

    impl Label {
        pub(super) fn into_name(self) -> String {
            match self {
                Self::Object { name } | Self::Name(name) => name,
            }
        }
    }

    #[derive(Deserialize)]
    pub(super) struct Repo {
        pub(super) full_name: String,
        #[serde(default)]
        pub(super) description: Option<String>,
        #[serde(default)]
        pub(super) private: bool,
        #[serde(default)]
        pub(super) default_branch: Option<String>,
        #[serde(default)]
        pub(super) archived: bool,
        #[serde(default)]
        pub(super) open_issues_count: u64,
        #[serde(default)]
        pub(super) pushed_at: Option<String>,
        #[serde(default)]
        pub(super) html_url: Option<String>,
    }

    #[derive(Deserialize)]
    pub(super) struct Issue {
        pub(super) number: u64,
        pub(super) title: String,
        pub(super) state: String,
        #[serde(default)]
        pub(super) user: Option<User>,
        #[serde(default)]
        pub(super) labels: Vec<Label>,
        #[serde(default)]
        pub(super) comments: u64,
        #[serde(default)]
        pub(super) updated_at: Option<String>,
        #[serde(default)]
        pub(super) html_url: Option<String>,
        #[serde(default)]
        pub(super) body: Option<String>,
        /// Present, with any content, when the issue is a pull request.
        #[serde(default)]
        pub(super) pull_request: Option<serde::de::IgnoredAny>,
    }

    #[derive(Deserialize)]
    pub(super) struct Branch {
        #[serde(rename = "ref")]
        pub(super) name: String,
    }

    #[derive(Deserialize)]
    pub(super) struct Pull {
        pub(super) number: u64,
        pub(super) title: String,
        pub(super) state: String,
        #[serde(default)]
        pub(super) user: Option<User>,
        pub(super) head: Branch,
        pub(super) base: Branch,
        #[serde(default)]
        pub(super) draft: bool,
        #[serde(default)]
        pub(super) updated_at: Option<String>,
        #[serde(default)]
        pub(super) html_url: Option<String>,
        #[serde(default)]
        pub(super) body: Option<String>,
        #[serde(default)]
        pub(super) merged: Option<bool>,
        #[serde(default)]
        pub(super) mergeable: Option<bool>,
        #[serde(default)]
        pub(super) commits: Option<u64>,
        #[serde(default)]
        pub(super) additions: Option<u64>,
        #[serde(default)]
        pub(super) deletions: Option<u64>,
        #[serde(default)]
        pub(super) changed_files: Option<u64>,
    }

    #[derive(Deserialize)]
    pub(super) struct CheckRun {
        pub(super) name: String,
        pub(super) status: String,
        #[serde(default)]
        pub(super) conclusion: Option<String>,
        #[serde(default)]
        pub(super) html_url: Option<String>,
    }

    #[derive(Deserialize)]
    pub(super) struct CheckRuns {
        #[serde(default)]
        pub(super) total_count: Option<u64>,
        pub(super) check_runs: Vec<CheckRun>,
    }

    #[derive(Deserialize)]
    pub(super) struct Comment {
        pub(super) id: u64,
        #[serde(default)]
        pub(super) user: Option<User>,
        #[serde(default)]
        pub(super) created_at: Option<String>,
        #[serde(default)]
        pub(super) html_url: Option<String>,
    }

    #[derive(Deserialize)]
    pub(super) struct Merge {
        #[serde(default)]
        pub(super) sha: Option<String>,
        #[serde(default)]
        pub(super) merged: bool,
        #[serde(default)]
        pub(super) message: Option<String>,
    }
}

fn author(user: Option<wire::User>) -> String {
    user.map_or_else(|| GHOST.to_owned(), |user| user.login)
}

/// A repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "wire::Repo")]
pub struct RepoSummary {
    /// `owner/name`.
    pub full_name: String,
    /// The one-line description, if any.
    pub description: Option<String>,
    /// Whether it is private.
    pub private: bool,
    /// The branch pull requests merge into by default.
    pub default_branch: Option<String>,
    /// Whether it is archived, and so read-only.
    pub archived: bool,
    /// Open issues and pull requests, as GitHub counts them.
    pub open_issues: u64,
    /// When something was last pushed.
    pub pushed_at: Option<String>,
    /// Where a person would look at it.
    pub url: Option<String>,
}

impl From<wire::Repo> for RepoSummary {
    fn from(repo: wire::Repo) -> Self {
        Self {
            full_name: repo.full_name,
            description: repo.description,
            private: repo.private,
            default_branch: repo.default_branch,
            archived: repo.archived,
            open_issues: repo.open_issues_count,
            pushed_at: repo.pushed_at,
            url: repo.html_url,
        }
    }
}

impl RepoSummary {
    /// The repository as the model reads it.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = format!(
            "{} ({}{})\n",
            self.full_name,
            if self.private { "private" } else { "public" },
            if self.archived { ", archived" } else { "" }
        );
        line(&mut out, "description", self.description.as_deref());
        line(&mut out, "default branch", self.default_branch.as_deref());
        let _ = writeln!(out, "open issues and pull requests: {}", self.open_issues);
        line(&mut out, "last push", self.pushed_at.as_deref());
        line(&mut out, "url", self.url.as_deref());
        out
    }
}

/// An issue, as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "wire::Issue")]
pub struct IssueSummary {
    /// The issue number.
    pub number: u64,
    /// The title.
    pub title: String,
    /// `open` or `closed`.
    pub state: String,
    /// Who opened it.
    pub author: String,
    /// Label names.
    pub labels: Vec<String>,
    /// How many comments it has.
    pub comments: u64,
    /// Whether this is a pull request, which GitHub lists among issues.
    pub pull_request: bool,
    /// When it last changed.
    pub updated_at: Option<String>,
    /// Where a person would look at it.
    pub url: Option<String>,
}

impl From<wire::Issue> for IssueSummary {
    fn from(issue: wire::Issue) -> Self {
        IssueDetail::from(issue).summary
    }
}

impl IssueSummary {
    /// One line per issue, for a list.
    #[must_use]
    pub fn render_line(&self) -> String {
        let mut out = format!(
            "#{} [{}] {} (by {}",
            self.number, self.state, self.title, self.author
        );
        if self.pull_request {
            out.push_str(", pull request");
        }
        if !self.labels.is_empty() {
            let _ = write!(out, ", labels: {}", self.labels.join(", "));
        }
        let _ = write!(out, ", {} comments", self.comments);
        if let Some(updated) = &self.updated_at {
            let _ = write!(out, ", updated {updated}");
        }
        out.push(')');
        out
    }
}

/// An issue with its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "wire::Issue")]
pub struct IssueDetail {
    /// Everything a list shows.
    #[serde(flatten)]
    pub summary: IssueSummary,
    /// The body, empty when there is none.
    pub body: String,
}

impl From<wire::Issue> for IssueDetail {
    fn from(issue: wire::Issue) -> Self {
        Self {
            summary: IssueSummary {
                number: issue.number,
                title: issue.title,
                state: issue.state,
                author: author(issue.user),
                labels: issue
                    .labels
                    .into_iter()
                    .map(wire::Label::into_name)
                    .collect(),
                comments: issue.comments,
                pull_request: issue.pull_request.is_some(),
                updated_at: issue.updated_at,
                url: issue.html_url,
            },
            body: issue.body.unwrap_or_default(),
        }
    }
}

impl IssueDetail {
    /// The issue as the model reads it.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = self.summary.render_line();
        out.push('\n');
        line(&mut out, "url", self.summary.url.as_deref());
        out.push('\n');
        out.push_str(&self.body);
        out
    }
}

/// A pull request, as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "wire::Pull")]
pub struct PullSummary {
    /// The pull request number.
    pub number: u64,
    /// The title.
    pub title: String,
    /// `open` or `closed`.
    pub state: String,
    /// Who opened it.
    pub author: String,
    /// The branch it comes from.
    pub head: String,
    /// The branch it would merge into.
    pub base: String,
    /// Whether it is a draft.
    pub draft: bool,
    /// When it last changed.
    pub updated_at: Option<String>,
    /// Where a person would look at it.
    pub url: Option<String>,
}

impl From<wire::Pull> for PullSummary {
    fn from(pull: wire::Pull) -> Self {
        PullDetail::from(pull).summary
    }
}

impl PullSummary {
    /// One line per pull request, for a list.
    #[must_use]
    pub fn render_line(&self) -> String {
        let mut out = format!(
            "#{} [{}{}] {} ({} -> {}, by {}",
            self.number,
            self.state,
            if self.draft { ", draft" } else { "" },
            self.title,
            self.head,
            self.base,
            self.author
        );
        if let Some(updated) = &self.updated_at {
            let _ = write!(out, ", updated {updated}");
        }
        out.push(')');
        out
    }
}

/// A pull request with its body and size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "wire::Pull")]
pub struct PullDetail {
    /// Everything a list shows.
    #[serde(flatten)]
    pub summary: PullSummary,
    /// The body, empty when there is none.
    pub body: String,
    /// Whether it has been merged.
    pub merged: Option<bool>,
    /// Whether GitHub thinks it can merge cleanly; `None` while it is still
    /// working that out.
    pub mergeable: Option<bool>,
    /// Commits in it.
    pub commits: Option<u64>,
    /// Lines added.
    pub additions: Option<u64>,
    /// Lines removed.
    pub deletions: Option<u64>,
    /// Files changed.
    pub changed_files: Option<u64>,
}

impl From<wire::Pull> for PullDetail {
    fn from(pull: wire::Pull) -> Self {
        Self {
            summary: PullSummary {
                number: pull.number,
                title: pull.title,
                state: pull.state,
                author: author(pull.user),
                head: pull.head.name,
                base: pull.base.name,
                draft: pull.draft,
                updated_at: pull.updated_at,
                url: pull.html_url,
            },
            body: pull.body.unwrap_or_default(),
            merged: pull.merged,
            mergeable: pull.mergeable,
            commits: pull.commits,
            additions: pull.additions,
            deletions: pull.deletions,
            changed_files: pull.changed_files,
        }
    }
}

impl PullDetail {
    /// The pull request as the model reads it.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = self.summary.render_line();
        out.push('\n');
        if let Some(merged) = self.merged {
            let _ = writeln!(out, "merged: {merged}");
        }
        if let Some(mergeable) = self.mergeable {
            let _ = writeln!(out, "mergeable: {mergeable}");
        }
        if let (Some(files), Some(added), Some(removed)) =
            (self.changed_files, self.additions, self.deletions)
        {
            let _ = writeln!(out, "size: {files} files, +{added} -{removed}");
        }
        line(&mut out, "url", self.summary.url.as_deref());
        out.push('\n');
        out.push_str(&self.body);
        out
    }
}

/// One check run on a commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "wire::CheckRun")]
pub struct CheckSummary {
    /// The check's name.
    pub name: String,
    /// `queued`, `in_progress` or `completed`.
    pub status: String,
    /// How a completed check ended: `success`, `failure` and so on.
    pub conclusion: Option<String>,
    /// Where a person would look at it.
    pub url: Option<String>,
}

impl From<wire::CheckRun> for CheckSummary {
    fn from(run: wire::CheckRun) -> Self {
        Self {
            name: run.name,
            status: run.status,
            conclusion: run.conclusion,
            url: run.html_url,
        }
    }
}

impl CheckSummary {
    /// One line per check.
    #[must_use]
    pub fn render_line(&self) -> String {
        match &self.conclusion {
            Some(conclusion) => format!("{}: {} ({conclusion})", self.name, self.status),
            None => format!("{}: {}", self.name, self.status),
        }
    }
}

/// The check runs on a commit, and how many there are in all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "wire::CheckRuns")]
pub struct CheckList {
    /// How many runs GitHub has for the commit, which can exceed what was
    /// returned.
    pub total: Option<u64>,
    /// The runs returned.
    pub checks: Vec<CheckSummary>,
}

impl From<wire::CheckRuns> for CheckList {
    fn from(runs: wire::CheckRuns) -> Self {
        Self {
            total: runs.total_count,
            checks: runs.check_runs.into_iter().map(Into::into).collect(),
        }
    }
}

/// A comment the call posted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "wire::Comment")]
pub struct CommentSummary {
    /// The comment's id.
    pub id: u64,
    /// Who it was posted as.
    pub author: String,
    /// When.
    pub created_at: Option<String>,
    /// Where a person would look at it.
    pub url: Option<String>,
}

impl From<wire::Comment> for CommentSummary {
    fn from(comment: wire::Comment) -> Self {
        Self {
            id: comment.id,
            author: author(comment.user),
            created_at: comment.created_at,
            url: comment.html_url,
        }
    }
}

/// What a merge did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "wire::Merge")]
pub struct MergeResult {
    /// Whether it merged.
    pub merged: bool,
    /// The merge commit.
    pub sha: Option<String>,
    /// GitHub's account of it.
    pub message: Option<String>,
}

impl From<wire::Merge> for MergeResult {
    fn from(merge: wire::Merge) -> Self {
        Self {
            merged: merge.merged,
            sha: merge.sha,
            message: merge.message,
        }
    }
}

fn line(out: &mut String, name: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        let _ = writeln!(out, "{name}: {value}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_issue_keeps_what_it_names_and_drops_the_rest() {
        let payload = serde_json::json!({
            "url": "https://api.github.com/repos/acme/widgets/issues/412",
            "repository_url": "https://api.github.com/repos/acme/widgets",
            "events_url": "https://api.github.com/repos/acme/widgets/issues/412/events",
            "html_url": "https://github.com/acme/widgets/issues/412",
            "number": 412,
            "title": "Scheduler fires twice",
            "state": "open",
            "user": {"login": "octo", "id": 1, "avatar_url": "https://avatars.example/1"},
            "labels": [{"name": "bug", "color": "f00"}, "triage"],
            "comments": 3,
            "updated_at": "2026-10-01T12:00:00Z",
            "body": "Reproduced on 0.2.0",
            "reactions": {"+1": 4},
            "author_association": "MEMBER",
        });
        let issue: IssueDetail = serde_json::from_value(payload).unwrap();
        assert_eq!(issue.summary.labels, vec!["bug", "triage"]);
        assert_eq!(issue.summary.author, "octo");
        assert!(!issue.summary.pull_request);

        let projected = serde_json::to_value(&issue).unwrap();
        let mut keys: Vec<&str> = projected
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "author",
                "body",
                "comments",
                "labels",
                "number",
                "pull_request",
                "state",
                "title",
                "updated_at",
                "url"
            ]
        );
        let rendered = issue.render();
        assert!(!rendered.contains("api.github.com"), "{rendered}");
        assert!(!rendered.contains("avatars"), "{rendered}");
    }

    #[test]
    fn a_deleted_author_and_a_pull_request_listed_as_an_issue() {
        let issue: IssueSummary = serde_json::from_value(serde_json::json!({
            "number": 7, "title": "t", "state": "open", "user": null,
            "pull_request": {"url": "https://api.github.com/x"},
        }))
        .unwrap();
        assert_eq!(issue.author, "ghost");
        assert!(issue.pull_request);
        assert!(issue.render_line().contains("pull request"));
    }

    #[test]
    fn a_pull_request_reads_its_branches() {
        let pull: PullDetail = serde_json::from_value(serde_json::json!({
            "number": 88, "title": "Fix", "state": "open", "user": {"login": "a"},
            "head": {"ref": "fix/sched", "sha": "abc", "repo": {"full_name": "fork/widgets"}},
            "base": {"ref": "main", "sha": "def"},
            "draft": false, "merged": false, "mergeable": true,
            "additions": 10, "deletions": 2, "changed_files": 1, "commits": 1,
        }))
        .unwrap();
        assert_eq!(pull.summary.head, "fix/sched");
        assert_eq!(pull.summary.base, "main");
        assert!(pull.render().contains("fix/sched -> main"));
    }

    #[test]
    fn checks_keep_the_total_and_the_conclusion() {
        let list: CheckList = serde_json::from_value(serde_json::json!({
            "total_count": 2,
            "check_runs": [
                {"name": "ci", "status": "completed", "conclusion": "failure", "output": {"text": "x"}},
                {"name": "lint", "status": "in_progress", "conclusion": null},
            ],
        }))
        .unwrap();
        assert_eq!(list.total, Some(2));
        assert_eq!(list.checks[0].render_line(), "ci: completed (failure)");
        assert_eq!(list.checks[1].render_line(), "lint: in_progress");
    }
}
