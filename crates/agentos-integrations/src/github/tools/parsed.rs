//! A call's arguments, parsed and checked: the one form that is authorised,
//! recorded and executed.

use agentos_tools::{ToolError, parse_arguments};
use serde::de::DeserializeOwned;

use super::args::{
    ChecksArgs, CommentArgs, CreateIssueArgs, CreatePullArgs, IssueArgs, ListIssuesArgs,
    ListPullsArgs, MergeArgs, PullArgs, PullCommentArgs, RepoArgs, UpdateIssueArgs,
};
use super::operation::Operation;
use super::validate::{
    Repo, check_account, check_body, check_labels, check_limit, check_number, check_path,
    check_ref, check_title,
};

/// A call's arguments, parsed and checked.
#[derive(Debug)]
pub(super) enum Parsed {
    ReposGet(RepoArgs),
    IssuesList(ListIssuesArgs),
    IssuesGet(IssueArgs),
    IssuesCreate(CreateIssueArgs),
    IssuesComment(CommentArgs),
    IssuesUpdate(UpdateIssueArgs),
    PullsList(ListPullsArgs),
    PullsGet(PullArgs),
    PullsCreate(CreatePullArgs),
    PullsComment(PullCommentArgs),
    PullsMerge(MergeArgs),
    ChecksList(ChecksArgs),
}

/// `$body`, with `$a` bound to the arguments of whichever operation `$parsed`
/// holds, for the fields every argument type has.
macro_rules! each {
    ($parsed:expr, $a:ident => $body:expr) => {
        match $parsed {
            Parsed::ReposGet($a) => $body,
            Parsed::IssuesList($a) => $body,
            Parsed::IssuesGet($a) => $body,
            Parsed::IssuesCreate($a) => $body,
            Parsed::IssuesComment($a) => $body,
            Parsed::IssuesUpdate($a) => $body,
            Parsed::PullsList($a) => $body,
            Parsed::PullsGet($a) => $body,
            Parsed::PullsCreate($a) => $body,
            Parsed::PullsComment($a) => $body,
            Parsed::PullsMerge($a) => $body,
            Parsed::ChecksList($a) => $body,
        }
    };
}

impl Parsed {
    /// Parse and check `arguments` for `operation`, and the repository they
    /// name. The arguments are left with the repository in its canonical form.
    pub(super) fn new(
        operation: Operation,
        arguments: &serde_json::Value,
    ) -> Result<(Self, Repo), ToolError> {
        fn typed<T: DeserializeOwned>(
            operation: Operation,
            arguments: &serde_json::Value,
        ) -> Result<T, ToolError> {
            parse_arguments(operation.name(), arguments)
        }
        let (a, o) = (arguments, operation);
        let mut parsed = match operation {
            Operation::ReposGet => Self::ReposGet(typed(o, a)?),
            Operation::IssuesList => Self::IssuesList(typed(o, a)?),
            Operation::IssuesGet => Self::IssuesGet(typed(o, a)?),
            Operation::IssuesCreate => Self::IssuesCreate(typed(o, a)?),
            Operation::IssuesComment => Self::IssuesComment(typed(o, a)?),
            Operation::IssuesUpdate => Self::IssuesUpdate(typed(o, a)?),
            Operation::PullsList => Self::PullsList(typed(o, a)?),
            Operation::PullsGet => Self::PullsGet(typed(o, a)?),
            Operation::PullsCreate => Self::PullsCreate(typed(o, a)?),
            Operation::PullsComment => Self::PullsComment(typed(o, a)?),
            Operation::PullsMerge => Self::PullsMerge(typed(o, a)?),
            Operation::ChecksList => Self::ChecksList(typed(o, a)?),
        };
        let repo = parsed
            .check()
            .map_err(|message| ToolError::invalid(operation.name(), message))?;
        Ok((parsed, repo))
    }

    /// The account label the call names, if it names one.
    pub(super) fn account(&self) -> Option<&str> {
        each!(self, a => a.account.as_deref())
    }

    fn check(&mut self) -> Result<Repo, String> {
        let repo = Repo::parse(each!(self, a => &a.repo))?;
        each!(self, a => a.repo = repo.to_string());
        check_account(self.account())?;
        match self {
            Self::ReposGet(_) => Ok(()),
            Self::IssuesList(a) => {
                check_limit(a.limit)?;
                check_labels(&a.labels, true)
            }
            Self::IssuesGet(a) => check_number(a.number),
            Self::IssuesCreate(a) => {
                check_title(&a.title)?;
                check_body(&a.body, false)?;
                check_labels(&a.labels, false)
            }
            Self::IssuesComment(a) => {
                check_number(a.number)?;
                check_body(&a.body, true)
            }
            Self::IssuesUpdate(a) => {
                check_number(a.number)?;
                if a.state.is_none() && a.title.is_none() && a.body.is_none() && a.labels.is_none()
                {
                    return Err(
                        "nothing to change: give at least one of `state`, `title`, `body` or \
                         `labels`"
                            .into(),
                    );
                }
                if let Some(title) = &a.title {
                    check_title(title)?;
                }
                if let Some(body) = &a.body {
                    check_body(body, false)?;
                }
                if let Some(labels) = &a.labels {
                    check_labels(labels, false)?;
                }
                Ok(())
            }
            Self::PullsList(a) => check_limit(a.limit),
            Self::PullsGet(a) => check_number(a.number),
            Self::PullsCreate(a) => {
                check_ref("head branch", &a.head, true)?;
                check_ref("base branch", &a.base, false)?;
                check_title(&a.title)?;
                check_body(&a.body, false)
            }
            Self::PullsComment(a) => {
                check_number(a.number)?;
                check_body(&a.body, true)?;
                match (&a.path, a.line) {
                    (None, None) => Ok(()),
                    (Some(path), Some(line)) if line > 0 => check_path(path),
                    (Some(_), Some(_)) => Err("`line` starts at 1".into()),
                    _ => Err("`path` and `line` go together: give both or neither".into()),
                }
            }
            Self::PullsMerge(a) => {
                check_number(a.number)?;
                check_ref("base branch", &a.base, false)
            }
            Self::ChecksList(a) => {
                check_limit(a.limit)?;
                check_ref("git ref", &a.git_ref, false)
            }
        }?;
        Ok(repo)
    }

    /// The canonical validated form, which is what is authorised, recorded and
    /// executed.
    pub(super) fn to_value(&self, operation: Operation) -> Result<serde_json::Value, ToolError> {
        each!(self, a => serde_json::to_value(a))
            .map_err(|error| ToolError::invalid(operation.name(), error.to_string()))
    }
}
