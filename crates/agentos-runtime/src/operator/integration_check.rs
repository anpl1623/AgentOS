//! The check of a bound account: one authenticated read, `GET {host}/user`,
//! sent the way a tool's call as the account would be.
//!
//! Apart from binding because it is the one operator act that sends a token
//! anywhere, and what keeps that safe is here with it: the release of the
//! token recorded as a run's spend of it would be, the bounded read, and an
//! answer reported without quoting what the host sent back.

use std::sync::{Arc, PoisonError};
use std::time::Duration;

use agentos_audit::AuditLog;
use agentos_core::event::{AgentEvent, Event};
use agentos_core::ids::{AgentId, IntegrationAccountId, TaskId, TaskRunId};
use agentos_integrations::account::Account;
use agentos_tools::egress::{Egress, EgressRequest, Method, Url};
use agentos_tools::{CredentialResolver, Secret, ToolContext, ToolError};

use crate::credentials::SecretStoreResolver;
use crate::integrations::{CheckOutcome, IntegrationCheck, account_of};
use crate::{Runtime, RuntimeError};

impl Runtime {
    /// Make one authenticated read as an account, `GET {host}/user`, and say
    /// whether the host is the service's API and accepts the token.
    ///
    /// The request leaves through the same egress path, under the same
    /// address policy, as a tool's call as the account would, so a host the
    /// account cannot reach is reported as one here rather than at the first
    /// run. Nothing is sent when no token is stored.
    ///
    /// Sending the token is recorded as a run's spending of it is, as
    /// `network.credential.used`, naming [`CHECK_TOOL`] as what was given it,
    /// before it is released: the host is the operator's to type, and a
    /// mistyped one is somewhere the chain must show the token went.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if there is no such account, and
    /// [`RuntimeError::Audit`] if the spend could not be recorded, in which
    /// case nothing was sent. Every way the read itself can fail is an
    /// [`IntegrationCheck`], not an error.
    pub async fn test_integration(
        &self,
        account_id: IntegrationAccountId,
    ) -> Result<IntegrationCheck, RuntimeError> {
        let account = account_of(&self.database.integrations().get(account_id).await?);
        self.check_account(&account, Egress::new(account.address_policy()))
            .await
    }

    /// The read behind [`Self::test_integration`], through `egress`.
    pub(crate) async fn check_account(
        &self,
        account: &Account,
        egress: Egress,
    ) -> Result<IntegrationCheck, RuntimeError> {
        let check = |outcome, detail: String| Ok(IntegrationCheck { outcome, detail });
        let (base, credential) = match (account.base_url(), account.credential()) {
            (Ok(base), Ok(credential)) => (base.to_owned(), credential),
            (Err(error), _) | (_, Err(error)) => {
                return check(CheckOutcome::WrongHost, error.to_string());
            }
        };
        let Ok(url) = Url::parse(&format!("{base}/user")) else {
            return check(
                CheckOutcome::WrongHost,
                format!("`{base}/user` is not a URL a request can be made to"),
            );
        };

        let resolver = Arc::new(SecretStoreResolver::new(self.secrets.clone()));
        if resolver
            .resolve(&credential.origin, &credential.name)
            .await
            .is_none()
        {
            return check(
                CheckOutcome::Unauthorised,
                format!(
                    "no token is stored for `{}` at {}, so nothing was sent; remove the account \
                     and bind it again",
                    account.label, credential.origin
                ),
            );
        }

        let release = Arc::new(RecordedRelease {
            store: SecretStoreResolver::new(self.secrets.clone()),
            audit: Arc::clone(&self.audit),
            failure: std::sync::Mutex::new(None),
        });
        let context = ToolContext::new(
            AgentId::new(),
            TaskId::new(),
            TaskRunId::new(),
            self.config.workspace.clone(),
        )
        .with_credentials(release.clone());
        let request = EgressRequest::new(Method::GET, url)
            .with_header("Accept", "application/vnd.github+json")
            .with_header("X-GitHub-Api-Version", "2022-11-28")
            .with_credential(credential)
            .with_timeout(CHECK_TIMEOUT)
            .with_max_bytes(CHECK_MAX_BYTES);

        let outcome = match egress.send(&context, request).await {
            Ok(response) => classify(&base, response.status, response.text()),
            Err(ToolError::Denied { reason }) => IntegrationCheck {
                outcome: CheckOutcome::WrongHost,
                detail: format!(
                    "refused before connecting: {reason}{}",
                    if account.private_network {
                        ""
                    } else {
                        ". An Enterprise server on a private network needs \
                         `--allow-private-network` when it is bound"
                    }
                ),
            },
            Err(error) => IntegrationCheck {
                outcome: CheckOutcome::Unreachable,
                detail: error.to_string(),
            },
        };
        // A token withheld because its spend could not be recorded failed the
        // read as a missing one would; the operator is told the real reason.
        let failure = release
            .failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match failure {
            Some(error) => Err(error),
            None => Ok(outcome),
        }
    }
}

/// The secret store as an account check spends from it.
///
/// Each release is recorded as `network.credential.used` before the value is
/// handed over, as the pipeline records a run's, and at the same moment: when
/// the egress path asks, after the address has been checked, so a host that
/// is refused shows no spend. A release that cannot be recorded is not made,
/// and the failure is kept for the check to report.
#[derive(Debug)]
struct RecordedRelease {
    store: SecretStoreResolver,
    audit: Arc<AuditLog>,
    failure: std::sync::Mutex<Option<RuntimeError>>,
}

#[async_trait::async_trait]
impl CredentialResolver for RecordedRelease {
    async fn resolve(&self, origin: &str, name: &str) -> Option<Secret> {
        let secret = self.store.resolve(origin, name).await?;
        let recorded = self
            .audit
            .record(Event::new(AgentEvent::CredentialUsed {
                origin: origin.to_owned(),
                name: name.to_owned(),
                tool: CHECK_TOOL.to_owned(),
            }))
            .await;
        match recorded {
            Ok(_) => Some(secret),
            Err(error) => {
                *self.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(error.into());
                None
            }
        }
    }
}

/// What an account check's spend of the token is recorded as having been
/// given to: the operator's check, not a tool a run could call.
pub const CHECK_TOOL: &str = "operator.integration.test";

/// How long an account check waits for the host, which is less than a run's
/// request: an operator is watching.
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// How much of the check's answer is read. A user record is a few hundred
/// bytes; anything near this is not one.
const CHECK_MAX_BYTES: usize = 64 * 1024;

/// What a host's answer to `GET {base}/user` says about the account.
///
/// Only the status and, on success, the login are used. The body is never
/// quoted: it is the host's text, and a hostile host can put anything in it,
/// the token it was just sent included.
fn classify(base: &str, status: u16, body: Option<&str>) -> IntegrationCheck {
    let (outcome, detail) = match status {
        200..=299 => match body.and_then(login) {
            Some(login) => (CheckOutcome::Reachable, format!("authenticated as {login}")),
            None => (
                CheckOutcome::WrongHost,
                format!(
                    "the host answered {status}, but not as the GitHub API does; for an \
                     Enterprise server the host ends in `/api/v3`"
                ),
            ),
        },
        300..=399 => (
            CheckOutcome::WrongHost,
            format!(
                "the host redirects ({status}), and redirects are not followed: bind the API's \
                 own address"
            ),
        ),
        401 => (
            CheckOutcome::Unauthorised,
            "the host refused the token (401): it may be mistyped, expired or revoked".to_owned(),
        ),
        403 => (
            CheckOutcome::Unauthorised,
            "the host refused the token (403): it may lack a scope, or the account is rate \
             limited"
                .to_owned(),
        ),
        404 => (
            CheckOutcome::WrongHost,
            format!(
                "`{base}/user` does not exist there (404); for an Enterprise server the host is \
                 usually `https://<server>/api/v3`"
            ),
        ),
        500..=599 => (
            CheckOutcome::Unreachable,
            format!("the host answered {status}: it is up, and not serving the API"),
        ),
        _ => (
            CheckOutcome::WrongHost,
            format!("the host answered {status}, which the GitHub API does not"),
        ),
    };
    IntegrationCheck { outcome, detail }
}

/// The `login` of a user record, if it is one a GitHub account could have:
/// 1 to 39 letters, digits or `-`. Anything else is not reported, since it is
/// not a login and could be whatever the host chose to send.
fn login(body: &str) -> Option<String> {
    let user: serde_json::Value = serde_json::from_str(body).ok()?;
    let login = user.get("login")?.as_str()?;
    (!login.is_empty()
        && login.len() <= 39
        && login
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
    .then(|| login.to_owned())
}

#[cfg(test)]
mod tests {
    //! The account check against a server on loopback, which only a test can
    //! reach: [`Runtime::test_integration`] builds its transport from the
    //! account alone, and no account admits loopback.

    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    use agentos_secrets::InMemorySecretStore;

    use super::*;

    const TOKEN: &str = "ghp_EXAMPLETOKEN0123456789abcdefABCDEF";

    /// A loopback server that answers every request with `status` and a body
    /// built from the `Authorization` header it was sent, as a hostile or
    /// careless host might. Returns its origin and every request head.
    fn server(status: &'static str, body: fn(&str) -> String) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let origin = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
                    head.push_str(&line);
                    line.clear();
                }
                let authorization = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("authorization")
                            .then(|| value.trim().to_owned())
                    })
                    .unwrap_or_default();
                log.lock().unwrap().push(head);
                let body = body(&authorization);
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });
        (origin, seen)
    }

    async fn check_against(
        status: &'static str,
        body: fn(&str) -> String,
    ) -> (IntegrationCheck, Vec<String>) {
        let guard = tempfile::TempDir::new().unwrap();
        let root = std::fs::canonicalize(guard.path()).unwrap();
        let runtime = Runtime::in_memory(root, Arc::new(InMemorySecretStore::new()))
            .await
            .unwrap();
        let (host, seen) = server(status, body);
        let id = runtime
            .bind_integration(
                "github",
                "work",
                Some(&host),
                false,
                None,
                Secret::new(TOKEN),
            )
            .await
            .unwrap()
            .id;
        let account = account_of(&runtime.database.integrations().get(id).await.unwrap());
        let egress = Egress::for_tests_admitting_loopback(account.address_policy());
        let check = runtime.check_account(&account, egress).await.unwrap();
        assert!(!check.detail.contains("EXAMPLETOKEN"), "{}", check.detail);
        let heads = seen.lock().unwrap().clone();

        // The token was sent, so its spend is in the chain: once per check,
        // naming the check rather than a tool, and never the value.
        let records = runtime.database.audit_sink().all().await.unwrap();
        let spent: Vec<_> = records
            .iter()
            .filter(|record| record.kind == "network.credential.used")
            .collect();
        assert_eq!(spent.len(), heads.len(), "one record per token sent");
        for record in &spent {
            assert_eq!(record.payload["tool"], CHECK_TOOL);
            assert_eq!(record.payload["name"], "work");
            assert_eq!(record.payload["origin"], host.as_str());
        }
        let chain: String = records
            .iter()
            .map(|record| record.payload.to_string())
            .collect();
        assert!(!chain.contains("EXAMPLETOKEN"), "{chain}");
        assert!(runtime.verify_audit().await.unwrap().is_intact());
        (check, heads)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_host_that_knows_the_token_is_reachable_and_was_asked_once() {
        let (check, heads) =
            check_against("200 OK", |_| r#"{"login":"octocat","id":1}"#.to_owned()).await;
        assert_eq!(check.outcome, CheckOutcome::Reachable);
        assert_eq!(check.detail, "authenticated as octocat");
        assert_eq!(heads.len(), 1, "one read, no retry");
        assert!(heads[0].starts_with("GET /user "), "{}", heads[0]);
        assert!(
            heads[0].contains(&format!("Bearer {TOKEN}")),
            "{}",
            heads[0]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_refusal_that_echoes_the_token_is_reported_without_it() {
        let echo =
            |authorization: &str| format!(r#"{{"message":"Bad credentials: {authorization}"}}"#);
        let (check, _) = check_against("401 Unauthorized", echo).await;
        assert_eq!(check.outcome, CheckOutcome::Unauthorised);
        let (check, _) = check_against("403 Forbidden", echo).await;
        assert_eq!(check.outcome, CheckOutcome::Unauthorised);
        let (check, _) = check_against("500 Internal Server Error", echo).await;
        assert_eq!(check.outcome, CheckOutcome::Unreachable);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn something_other_than_the_api_is_the_wrong_host() {
        let (check, _) = check_against("404 Not Found", |_| "{}".to_owned()).await;
        assert_eq!(check.outcome, CheckOutcome::WrongHost);
        assert!(check.detail.contains("/api/v3"), "{}", check.detail);
        // A success that is not a user record, such as a web page.
        let (check, _) = check_against("200 OK", |_| "<html></html>".to_owned()).await;
        assert_eq!(check.outcome, CheckOutcome::WrongHost);
        // A "login" that is not one is not repeated, whatever it holds.
        let (check, _) = check_against("200 OK", |authorization| {
            format!(
                r#"{{"login":"{}"}}"#,
                authorization.trim_start_matches("Bearer ")
            )
        })
        .await;
        assert_eq!(check.outcome, CheckOutcome::WrongHost);
        let (check, _) = check_against("301 Moved Permanently", |_| String::new()).await;
        assert_eq!(check.outcome, CheckOutcome::WrongHost);
    }
}
