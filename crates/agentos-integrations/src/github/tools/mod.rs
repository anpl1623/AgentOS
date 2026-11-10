//! The twelve GitHub tools.
//!
//! One type, [`GitHubTool`], parameterised by [`Operation`], so that what is
//! the same for every tool — the account, the repository, the capability, the
//! provenance of what comes back — is written once and cannot drift between
//! them. What differs is the arguments, the plan's sentence and the requests,
//! and each of those is one arm of a match the compiler holds exhaustive.
//!
//! Each part of that is a module of its own: the operations and what each
//! needs (`operation`), the arguments (`args`), the checks every argument
//! passes before anything is planned (`validate`), the parsed call
//! (`parsed`), the sentence an approval card shows (`describe`), and the
//! requests and what is made of the answers (`requests`). The tool itself,
//! which holds them together, is here.

mod args;
mod describe;
mod operation;
mod parsed;
mod requests;
mod validate;

use std::sync::Arc;
use std::time::Duration;

use agentos_core::tool::ToolMetadata;
use agentos_tools::egress::Egress;
use agentos_tools::{Tool, ToolContext, ToolError, ToolOutput, ToolPlan};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::INTEGRATION;
use super::api::Client;
use crate::account::{Account, AccountDirectory};

pub use args::{
    ChecksArgs, CommentArgs, CreateIssueArgs, CreatePullArgs, DEFAULT_LIMIT, IssueArgs, IssueState,
    ListIssuesArgs, ListPullsArgs, ListState, MAX_LIMIT, MergeArgs, MergeMethod, PullArgs,
    PullCommentArgs, RepoArgs, UpdateIssueArgs,
};
pub use describe::{SUMMARY_EXCERPT_CHARS, excerpt};
pub use operation::Operation;
pub use validate::Repo;

use parsed::Parsed;
use requests::run;

/// Longest any one request is given, within the run's own budget.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A GitHub tool.
///
/// Holds the account directory, not an account: which account a call acts as
/// is resolved on every call, so binding or removing one changes nothing about
/// what is registered.
#[derive(Debug)]
pub struct GitHubTool {
    operation: Operation,
    metadata: ToolMetadata,
    egress: Egress,
    directory: Arc<dyn AccountDirectory>,
}

impl GitHubTool {
    /// The tool for `operation`.
    #[must_use]
    pub fn new(operation: Operation, egress: Egress, directory: Arc<dyn AccountDirectory>) -> Self {
        Self {
            operation,
            metadata: operation.metadata(),
            egress,
            directory,
        }
    }

    /// Which operation this is.
    #[must_use]
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    async fn account(&self, parsed: &Parsed) -> Result<Account, ToolError> {
        let account = self
            .directory
            .resolve(INTEGRATION, parsed.account())
            .await?;
        // Checked here as well as when it was bound: the row is the
        // operator's, but a host that does not read as one origin must not
        // be built on whoever wrote it.
        account.origin()?;
        Ok(account)
    }
}

#[async_trait]
impl Tool for GitHubTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        Parsed::new(self.operation, arguments)?
            .0
            .to_value(self.operation)
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        _context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        // Everything here comes from the arguments and the operator's own
        // account directory. Nothing asks GitHub whether the repository or the
        // issue exists: a plan that did would have had a side effect before it
        // was authorised, which is the one thing a plan must not have.
        let (parsed, repo) = Parsed::new(self.operation, arguments)?;
        // Resolved here to declare its credential and name it on the card, and
        // again when the call runs. A call that cannot run because no account
        // is bound fails now, rather than after somebody has approved it; an
        // account rebound in between has another credential, which the
        // pipeline will not release because this plan did not declare it.
        let account = self.account(&parsed).await?;
        let (summary, affected) = parsed.describe(&repo);
        let mut plan = ToolPlan::new(self.operation.risk(), summary)
            .requiring(repo.capability(self.operation.action()));
        for resource in affected {
            plan = plan.affecting(resource);
        }
        // The one capability above is the grant to act as the account on this
        // repository, so the credential is declared rather than asked about a
        // second time. The pipeline releases what is declared here and nothing
        // else, and the card shows whose credential it is.
        Ok(plan.spending(account.credential()?))
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let (parsed, repo) = Parsed::new(self.operation, &arguments)?;
        let account = self.account(&parsed).await?;
        let timeout = REQUEST_TIMEOUT.min(context.timeout);
        let client = Client::new(self.egress, &account, context, timeout);
        let output = tokio::select! {
            () = cancel.cancelled() => return Err(ToolError::Cancelled),
            output = run(&client, &parsed, &repo) => output?,
        };
        self.directory.record_use(&account).await;
        Ok(output)
    }
}
