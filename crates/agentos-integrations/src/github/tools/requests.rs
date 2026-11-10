//! The requests each operation makes, and what is made of the answers.
//!
//! Every answer is projected onto the narrow types in `model`, rendered for
//! the model, and labelled with the account it was read as and the endpoint
//! it came from, writes included: what GitHub returns was written by people
//! outside the run.

use std::fmt::Write as _;

use agentos_core::trust::DataSource;
use agentos_tools::{ToolError, ToolOutput};
use serde::{Deserialize, Serialize};

use super::parsed::Parsed;
use super::validate::Repo;
use crate::error::IntegrationError;
use crate::github::INTEGRATION;
use crate::github::api::{Call, Client, Reply};
use crate::github::model::{
    CheckList, CommentSummary, IssueDetail, IssueSummary, MergeResult, PullDetail, PullSummary,
    RepoSummary,
};

/// Room left after a diff for the line saying it was cut.
const DIFF_MARGIN_BYTES: usize = 256;

/// What one call produced, before it is labelled with its source.
struct Produced {
    /// The endpoint the content was read from, for provenance.
    endpoint: String,
    /// The text for the model.
    text: String,
    /// The same projection, for the UI.
    structured: serde_json::Value,
}

impl Produced {
    fn new(endpoint: String, text: String, structured: impl Serialize) -> Self {
        Self {
            endpoint,
            text,
            structured: serde_json::to_value(structured).unwrap_or(serde_json::Value::Null),
        }
    }
}

pub(super) async fn run(
    client: &Client<'_>,
    parsed: &Parsed,
    repo: &Repo,
) -> Result<ToolOutput, ToolError> {
    let produced = produce(client, parsed, repo).await?;
    // Every result names the account it was read as and the endpoint it came
    // from, writes included: the body of a created issue is GitHub's answer,
    // and GitHub is outside the runtime.
    Ok(ToolOutput::text(
        DataSource::integration(
            INTEGRATION,
            client.account().label.clone(),
            &produced.endpoint,
        ),
        produced.text,
    )
    .with_structured(produced.structured))
}

async fn produce(
    client: &Client<'_>,
    parsed: &Parsed,
    repo: &Repo,
) -> Result<Produced, IntegrationError> {
    let base = repo.path();
    Ok(match parsed {
        Parsed::ReposGet(_) => {
            let call = Call::get(base.clone());
            let repository: RepoSummary = client.json(&call, &repo.to_string()).await?;
            Produced::new(base, repository.render(), repository)
        }
        Parsed::IssuesList(a) => {
            let mut call = Call::get(format!("{base}/issues"))
                .query("state", a.state.as_str())
                .query("per_page", a.limit.to_string());
            if !a.labels.is_empty() {
                call = call.query("labels", a.labels.join(","));
            }
            let issues: Vec<IssueSummary> = client.json(&call, &repo.to_string()).await?;
            let text = render_list(
                &format!("{} issues in {repo}", a.state.as_str()),
                issues.iter().map(IssueSummary::render_line),
            );
            Produced::new(call.path().to_owned(), text, issues)
        }
        Parsed::IssuesGet(a) => {
            let call = Call::get(format!("{base}/issues/{}", a.number));
            let issue: IssueDetail = client.json(&call, &format!("{repo}#{}", a.number)).await?;
            Produced::new(call.path().to_owned(), issue.render(), issue)
        }
        Parsed::IssuesCreate(a) => {
            let call = Call::post(
                format!("{base}/issues"),
                serde_json::json!({"title": a.title, "body": a.body, "labels": a.labels}),
            );
            let issue: IssueDetail = client.json(&call, &repo.to_string()).await?;
            let text = format!(
                "opened {repo}#{}\n{}",
                issue.summary.number,
                issue.summary.render_line()
            );
            Produced::new(call.path().to_owned(), text, issue)
        }
        Parsed::IssuesComment(a) => {
            let call = Call::post(
                format!("{base}/issues/{}/comments", a.number),
                serde_json::json!({"body": a.body}),
            );
            let comment: CommentSummary =
                client.json(&call, &format!("{repo}#{}", a.number)).await?;
            let text = commented(repo, a.number, &comment);
            Produced::new(call.path().to_owned(), text, comment)
        }
        Parsed::IssuesUpdate(a) => {
            let mut patch = serde_json::Map::new();
            if let Some(state) = a.state {
                patch.insert("state".into(), state.as_str().into());
            }
            if let Some(title) = &a.title {
                patch.insert("title".into(), title.clone().into());
            }
            if let Some(body) = &a.body {
                patch.insert("body".into(), body.clone().into());
            }
            if let Some(labels) = &a.labels {
                patch.insert("labels".into(), labels.clone().into());
            }
            let call = Call::patch(
                format!("{base}/issues/{}", a.number),
                serde_json::Value::Object(patch),
            );
            let issue: IssueDetail = client.json(&call, &format!("{repo}#{}", a.number)).await?;
            let text = format!(
                "updated {repo}#{}\n{}",
                a.number,
                issue.summary.render_line()
            );
            Produced::new(call.path().to_owned(), text, issue)
        }
        Parsed::PullsList(a) => {
            let call = Call::get(format!("{base}/pulls"))
                .query("state", a.state.as_str())
                .query("per_page", a.limit.to_string());
            let pulls: Vec<PullSummary> = client.json(&call, &repo.to_string()).await?;
            let text = render_list(
                &format!("{} pull requests in {repo}", a.state.as_str()),
                pulls.iter().map(PullSummary::render_line),
            );
            Produced::new(call.path().to_owned(), text, pulls)
        }
        Parsed::PullsGet(a) => {
            let what = format!("{repo}#{}", a.number);
            let call = Call::get(format!("{base}/pulls/{}", a.number));
            let pull: PullDetail = client.json(&call, &what).await?;
            let mut text = pull.render();
            let mut structured = serde_json::to_value(&pull).unwrap_or_default();
            if a.include_diff {
                // Sized to what the pipeline will pass on, so that the line
                // saying where the diff was cut is inside what the model sees
                // rather than cut off with the rest.
                let room = client
                    .output_budget()
                    .saturating_sub(text.len() + DIFF_MARGIN_BYTES);
                let diff = client.send(&call.clone().diff(room), &what).await?;
                text.push_str("\n\n");
                text.push_str(&render_diff(&diff));
                if let serde_json::Value::Object(map) = &mut structured {
                    map.insert("diff_bytes".into(), diff.body.len().into());
                    map.insert("diff_truncated".into(), diff.truncated.into());
                }
            }
            Produced {
                endpoint: call.path().to_owned(),
                text,
                structured,
            }
        }
        Parsed::PullsCreate(a) => {
            let call = Call::post(
                format!("{base}/pulls"),
                serde_json::json!({
                    "head": a.head, "base": a.base, "title": a.title,
                    "body": a.body, "draft": a.draft,
                }),
            );
            let pull: PullDetail = client.json(&call, &repo.to_string()).await?;
            let text = format!(
                "opened {repo}#{}\n{}",
                pull.summary.number,
                pull.summary.render_line()
            );
            Produced::new(call.path().to_owned(), text, pull)
        }
        Parsed::PullsComment(a) => {
            let what = format!("{repo}#{}", a.number);
            let call = match (&a.path, a.line) {
                (Some(path), Some(line)) => {
                    // A comment on a line is anchored to a commit, and the
                    // one that matters is the head the reviewer is looking at.
                    let pull: PullHead = client
                        .json(&Call::get(format!("{base}/pulls/{}", a.number)), &what)
                        .await?;
                    Call::post(
                        format!("{base}/pulls/{}/comments", a.number),
                        serde_json::json!({
                            "body": a.body, "commit_id": pull.head.sha,
                            "path": path, "line": line, "side": "RIGHT",
                        }),
                    )
                }
                // The conversation on a pull request is its issue's.
                _ => Call::post(
                    format!("{base}/issues/{}/comments", a.number),
                    serde_json::json!({"body": a.body}),
                ),
            };
            let comment: CommentSummary = client.json(&call, &what).await?;
            let text = commented(repo, a.number, &comment);
            Produced::new(call.path().to_owned(), text, comment)
        }
        Parsed::PullsMerge(a) => {
            let what = format!("{repo}#{}", a.number);
            let pull_call = Call::get(format!("{base}/pulls/{}", a.number));
            let pull: PullHead = client.json(&pull_call, &what).await?;
            // The operator approved a merge into the branch the card named.
            // A pull request retargeted since, or one the model described
            // wrongly, is refused here rather than merged somewhere else.
            if pull.base.name != a.base {
                return Err(IntegrationError::Api {
                    status: 409,
                    message: format!(
                        "{what} merges into `{}`, not `{}`; nothing was merged",
                        pull.base.name, a.base
                    ),
                });
            }
            if pull.state != "open" {
                return Err(IntegrationError::Api {
                    status: 409,
                    message: format!("{what} is {}; nothing was merged", pull.state),
                });
            }
            // Pinned to the head that was just checked, so a push between the
            // check and the merge makes GitHub refuse rather than merge it.
            let call = Call::put(
                format!("{base}/pulls/{}/merge", a.number),
                serde_json::json!({"merge_method": a.method.as_str(), "sha": pull.head.sha}),
            );
            let merge: MergeResult = client.json(&call, &what).await?;
            let text = if merge.merged {
                format!(
                    "merged {what} ({}) into {}{}",
                    a.method.as_str(),
                    a.base,
                    merge
                        .sha
                        .as_deref()
                        .map(|sha| format!(" as {sha}"))
                        .unwrap_or_default()
                )
            } else {
                format!(
                    "{what} was not merged: {}",
                    merge.message.as_deref().unwrap_or("GitHub gave no reason")
                )
            };
            Produced::new(call.path().to_owned(), text, merge)
        }
        Parsed::ChecksList(a) => {
            let call = Call::get(format!(
                "{base}/commits/{}/check-runs",
                crate::github::api::encode_segment(&a.git_ref)
            ))
            .query("per_page", a.limit.to_string());
            let list: CheckList = client.json(&call, &format!("{repo}@{}", a.git_ref)).await?;
            let mut heading = format!("checks on {repo}@{}", a.git_ref);
            if let Some(total) = list.total.filter(|total| *total > list.checks.len() as u64) {
                let _ = write!(heading, " ({} of {total} shown)", list.checks.len());
            }
            let text = render_list(&heading, list.checks.iter().map(|c| c.render_line()));
            Produced::new(call.path().to_owned(), text, list)
        }
    })
}

/// The few fields of a pull request a write checks before acting.
#[derive(Deserialize)]
struct PullHead {
    state: String,
    head: Sha,
    base: BaseRef,
}

#[derive(Deserialize)]
struct Sha {
    sha: String,
}

#[derive(Deserialize)]
struct BaseRef {
    #[serde(rename = "ref")]
    name: String,
}

fn commented(repo: &Repo, number: u64, comment: &CommentSummary) -> String {
    let mut text = format!("commented on {repo}#{number} as {}", comment.author);
    if let Some(url) = &comment.url {
        let _ = write!(text, "\n{url}");
    }
    text
}

fn render_list(heading: &str, lines: impl Iterator<Item = String>) -> String {
    let lines: Vec<String> = lines.collect();
    if lines.is_empty() {
        return format!("no {heading}");
    }
    let mut text = format!("{heading}:\n");
    for line in lines {
        text.push_str(&line);
        text.push('\n');
    }
    text
}

/// The diff, with a line saying where it was cut when it was.
///
/// A model reasoning about half a diff it believes is whole is worse off than
/// one told it is missing something.
fn render_diff(diff: &Reply) -> String {
    if let Some(bytes) = diff.unread {
        // Not reached while the diff is requested with `accepting_diff`, under
        // which the egress path reads the first part of a body declared
        // longer than the limit rather than none of it. Kept so that a change
        // there shows as a marked omission, not as an empty diff.
        return format!("diff: [not shown: {bytes} bytes, over the limit for one call]");
    }
    let mut text = format!("diff:\n{}", diff.body);
    if diff.truncated {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        let _ = write!(text, "[diff truncated at {} bytes]", diff.body.len());
    }
    text
}
