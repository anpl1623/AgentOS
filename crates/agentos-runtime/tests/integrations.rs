//! Integration accounts: bound by the operator as a network credential and a
//! row, recorded as both, and found by the registry's tools on every call.
//!
//! The CLI and the desktop bind, unbind, list and test through these methods,
//! so these are the runtime-level halves of what both clients promise. What
//! a GitHub call does once it has an account is the integration crate's to
//! test, against a hostile fixture; what is tested here is that the account
//! it gets is the operator's, with the operator's host and address policy.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::TcpListener;
use std::sync::Arc;

use agentos_audit::AuditRecord;
use agentos_core::ids::{AgentId, IntegrationAccountId, TaskId, TaskRunId};
use agentos_integrations::account::AccountDirectory;
use agentos_runtime::integrations::{CheckOutcome, INTEGRATIONS};
use agentos_runtime::{Runtime, RuntimeConfig, RuntimeError, SqliteAccountDirectory};
use agentos_secrets::{InMemorySecretStore, SecretStore, network_key};
use agentos_tools::egress::AddressPolicy;
use agentos_tools::{Secret, ToolContext};
use tempfile::TempDir;

const TOKEN: &str = "ghp_EXAMPLETOKEN0123456789abcdefABCDEF";

async fn runtime() -> (Runtime, Arc<InMemorySecretStore>, TempDir) {
    let guard = TempDir::new().unwrap();
    let root = std::fs::canonicalize(guard.path()).unwrap();
    let secrets = Arc::new(InMemorySecretStore::new());
    let runtime = Runtime::in_memory(root, secrets.clone()).await.unwrap();
    (runtime, secrets, guard)
}

async fn records(runtime: &Runtime) -> Vec<AuditRecord> {
    runtime.database().audit_sink().all().await.unwrap()
}

fn kinds(records: &[AuditRecord]) -> Vec<&str> {
    records.iter().map(|record| record.kind.as_str()).collect()
}

/// Everything the database holds, as text: the audit chain, every setting and
/// every account row.
async fn everything_stored(runtime: &Runtime) -> String {
    let mut text = String::new();
    for record in records(runtime).await {
        text.push_str(&record.payload.to_string());
    }
    for (key, value) in runtime.database().settings().all().await.unwrap() {
        text.push_str(&key);
        text.push_str(&value);
    }
    for account in runtime.database().integrations().list().await.unwrap() {
        text.push_str(&format!("{account:?}"));
    }
    text
}

async fn bind(
    runtime: &Runtime,
    label: &str,
    host: Option<&str>,
    private_network: bool,
) -> IntegrationAccountId {
    runtime
        .bind_integration(
            "github",
            label,
            host,
            private_network,
            Some("repo, read:org"),
            Secret::new(TOKEN),
        )
        .await
        .unwrap()
        .id
}

fn context(dir: &TempDir) -> ToolContext {
    ToolContext::new(
        AgentId::new(),
        TaskId::new(),
        TaskRunId::new(),
        dir.path().to_path_buf(),
    )
}

#[tokio::test]
async fn binding_stores_the_token_as_a_network_credential_and_records_both_acts() {
    let (runtime, secrets, _guard) = runtime().await;
    let id = bind(&runtime, "work", None, false).await;

    // The token is the network credential for the default host's origin,
    // named by the label: the same credential `network.request` would spend.
    let key = network_key("https://api.github.com", "work").unwrap();
    assert_eq!(secrets.get(&key).unwrap().expose(), TOKEN);
    assert_eq!(
        runtime.list_network_credentials().await.unwrap(),
        vec![("https://api.github.com".to_owned(), "work".to_owned())]
    );

    // The credential first, then the row, each recorded as itself.
    let all = records(&runtime).await;
    assert_eq!(
        kinds(&all),
        vec!["operator.credential.set", "operator.integration.bound"]
    );
    let bound = &all[1].payload;
    assert_eq!(bound["integration"], "github");
    assert_eq!(bound["label"], "work");
    assert_eq!(bound["host"], "https://api.github.com");
    assert_eq!(bound["private_network"], false);

    let listed = runtime.list_integrations().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].account.id, id);
    assert_eq!(listed[0].account.scopes.as_deref(), Some("repo, read:org"));
    assert_eq!(listed[0].origin.as_deref(), Some("https://api.github.com"));
    assert!(listed[0].credential_present);

    // Nothing the database holds carries the token or any part of it.
    let stored = everything_stored(&runtime).await;
    assert!(!stored.contains("EXAMPLETOKEN"), "{stored}");
    assert!(runtime.verify_audit().await.unwrap().is_intact());
}

#[tokio::test]
async fn an_enterprise_account_reaches_a_private_network_only_by_the_operators_flag() {
    let (runtime, secrets, _guard) = runtime().await;
    bind(
        &runtime,
        "work",
        Some(" https://GHE.example/api/v3/ "),
        true,
    )
    .await;
    bind(&runtime, "public", None, false).await;

    // Stored as the canonical origin and the path, less the slash; the token
    // is under that origin, which is what a request is matched against.
    let key = network_key("https://ghe.example", "work").unwrap();
    assert_eq!(secrets.get(&key).unwrap().expose(), TOKEN);
    let bound: Vec<AuditRecord> = records(&runtime)
        .await
        .into_iter()
        .filter(|record| record.kind == "operator.integration.bound")
        .collect();
    assert_eq!(bound[0].payload["host"], "https://ghe.example/api/v3");
    assert_eq!(bound[0].payload["private_network"], true);
    assert_eq!(bound[1].payload["private_network"], false);

    // What a tool is handed is the row: its host and its address policy, set
    // where the record above says they were and nowhere else.
    let directory = runtime.integration_accounts();
    let work = directory.resolve("github", Some("work")).await.unwrap();
    assert_eq!(work.host, "https://ghe.example/api/v3");
    assert_eq!(work.address_policy(), AddressPolicy::AllowPrivateNetwork);
    let public = directory.resolve("github", Some("public")).await.unwrap();
    assert_eq!(public.address_policy(), AddressPolicy::Strict);

    // And no argument a model writes can say otherwise: a call naming a host
    // or an address policy is not a call any GitHub tool accepts.
    let registry = runtime.registry();
    let tool = registry.get("github.issues.get").unwrap();
    for smuggled in [
        serde_json::json!({"repo": "acme/widgets", "number": 1, "host": "http://169.254.169.254"}),
        serde_json::json!({"repo": "acme/widgets", "number": 1, "private_network": true}),
        serde_json::json!({"repo": "acme/widgets", "number": 1, "url": "https://evil.example"}),
    ] {
        assert!(tool.validate(&smuggled).is_err(), "{smuggled}");
    }
}

#[tokio::test]
async fn a_binding_that_cannot_be_made_writes_and_records_nothing() {
    let (runtime, secrets, _guard) = runtime().await;
    bind(&runtime, "work", None, false).await;
    let before = records(&runtime).await.len();

    for (integration, label, host, token, says) in [
        ("gitlab", "work", None, TOKEN, "unknown integration"),
        ("github", "Work", None, TOKEN, "account label"),
        ("github", "work.token", None, TOKEN, "account label"),
        ("github", "", None, TOKEN, "account label"),
        ("github", "ops", Some("ftp://ghe.example"), TOKEN, "http"),
        (
            "github",
            "ops",
            Some("https://u:p@ghe.example"),
            TOKEN,
            "cannot be used",
        ),
        (
            "github",
            "ops",
            Some("https://ghe.example/api?x=1"),
            TOKEN,
            "query",
        ),
        ("github", "ops", None, "   ", "no secret"),
        (
            "github",
            "ops",
            None,
            "tok\r\nX-Injected: 1",
            "control characters",
        ),
        // Rebinding a label would replace the token the bound account uses.
        ("github", "work", None, "another-token", "already bound"),
    ] {
        let refused = runtime
            .bind_integration(integration, label, host, false, None, Secret::new(token))
            .await;
        let Err(RuntimeError::Rejected(message)) = refused else {
            panic!("{integration} / {label} was not refused: {refused:?}");
        };
        assert!(
            message.contains(says),
            "{integration} / {label}: `{message}` does not say `{says}`"
        );
    }

    // A refused label is not quoted back: it may be the token.
    let refused = runtime
        .bind_integration("github", TOKEN, None, false, None, Secret::new(TOKEN))
        .await
        .unwrap_err()
        .to_string();
    assert!(!refused.contains("EXAMPLETOKEN"), "{refused}");

    assert_eq!(records(&runtime).await.len(), before);
    assert_eq!(runtime.list_integrations().await.unwrap().len(), 1);
    let key = network_key("https://api.github.com", "work").unwrap();
    assert_eq!(secrets.get(&key).unwrap().expose(), TOKEN);
    assert!(
        secrets
            .get(&network_key("https://api.github.com", "ops").unwrap())
            .is_err()
    );
}

#[tokio::test]
async fn a_token_pasted_into_the_scopes_note_is_refused_and_nothing_is_kept() {
    let (runtime, secrets, _guard) = runtime().await;
    // A token in the note's alphabet gets past the shape of the note, and is
    // caught by what it shares with the token: the whole of it, or any run
    // long enough that the pipeline would redact it.
    let lower = "ghp_lowercase_token_0123456789";
    for note in [lower, "repo, lowercase_token, read:org"] {
        let refused = runtime
            .bind_integration(
                "github",
                "work",
                None,
                false,
                Some(note),
                Secret::new(lower),
            )
            .await;
        let Err(RuntimeError::Rejected(message)) = refused else {
            panic!("`{note}` was kept: {refused:?}");
        };
        assert!(message.contains("scopes note"), "{message}");
        assert!(!message.contains("lowercase_token"), "{message}");
    }
    // And one in a token's own alphabet is refused for its shape.
    let refused = runtime
        .bind_integration(
            "github",
            "work",
            None,
            false,
            Some(TOKEN),
            Secret::new(TOKEN),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("scopes note"), "{refused}");
    assert!(!refused.contains("EXAMPLETOKEN"), "{refused}");

    assert!(records(&runtime).await.is_empty());
    assert!(runtime.list_integrations().await.unwrap().is_empty());
    assert!(runtime.list_network_credentials().await.unwrap().is_empty());
    assert!(
        secrets
            .get(&network_key("https://api.github.com", "work").unwrap())
            .is_err()
    );
    let stored = everything_stored(&runtime).await;
    assert!(!stored.contains("lowercase_token"), "{stored}");
}

#[tokio::test]
async fn a_host_written_in_capitals_binds_as_the_origin_it_is() {
    // Accepted by the runtime's reading of an origin and refused by the
    // table's, this used to store the token, record it, and then fail on the
    // row: an orphan secret, replacing whatever was stored there before.
    let (runtime, secrets, _guard) = runtime().await;
    let account = runtime
        .bind_integration(
            "github",
            "upper",
            Some("HTTPS://API.github.com"),
            false,
            None,
            Secret::new(TOKEN),
        )
        .await
        .unwrap();
    assert_eq!(account.host, "https://api.github.com");
    let listed = runtime.list_integrations().await.unwrap();
    assert_eq!(listed[0].account.host, "https://api.github.com");
    assert!(listed[0].credential_present);
    let key = network_key("https://api.github.com", "upper").unwrap();
    assert_eq!(secrets.get(&key).unwrap().expose(), TOKEN);
    assert_eq!(
        kinds(&records(&runtime).await),
        vec!["operator.credential.set", "operator.integration.bound"]
    );
}

#[tokio::test]
async fn binding_over_a_network_credential_no_account_uses_is_refused() {
    // Stored by the operator for `network.request`. Binding over it would
    // replace it without a word, and unbinding would then delete it.
    let (runtime, secrets, _guard) = runtime().await;
    runtime
        .set_network_credential("https://api.github.com", "work", "the-operators-own")
        .await
        .unwrap();
    let before = records(&runtime).await.len();

    let refused = runtime
        .check_binding("github", "work", None, false, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("already stored"), "{refused}");
    let refused = runtime
        .bind_integration("github", "work", None, false, None, Secret::new(TOKEN))
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("agentos credential remove"), "{refused}");

    let key = network_key("https://api.github.com", "work").unwrap();
    assert_eq!(secrets.get(&key).unwrap().expose(), "the-operators-own");
    assert_eq!(records(&runtime).await.len(), before);
    assert!(runtime.list_integrations().await.unwrap().is_empty());

    // The same name at another origin is another credential, and no obstacle.
    bind(&runtime, "work", Some("https://ghe.example/api/v3"), false).await;
    assert_eq!(secrets.get(&key).unwrap().expose(), "the-operators-own");
}

#[tokio::test]
async fn a_private_network_is_refused_on_the_services_own_host() {
    let (runtime, _secrets, _guard) = runtime().await;
    let refused = runtime
        .bind_integration("github", "work", None, true, None, Secret::new(TOKEN))
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("self-hosted"), "{refused}");
    assert!(records(&runtime).await.is_empty());
    assert!(runtime.list_network_credentials().await.unwrap().is_empty());
}

#[tokio::test]
async fn unbinding_removes_the_row_and_then_the_token_and_records_both() {
    let (runtime, secrets, _guard) = runtime().await;
    let id = bind(&runtime, "work", None, false).await;
    let before = records(&runtime).await.len();

    let removed = runtime.unbind_integration(id).await.unwrap();
    assert_eq!(removed.label, "work");
    assert!(runtime.list_integrations().await.unwrap().is_empty());
    assert!(
        secrets
            .get(&network_key("https://api.github.com", "work").unwrap())
            .is_err()
    );
    assert!(runtime.list_network_credentials().await.unwrap().is_empty());

    // The row's record first: that is the moment no call can act as it.
    let all = records(&runtime).await;
    assert_eq!(
        kinds(&all[before..]),
        vec![
            "operator.integration.unbound",
            "operator.credential.removed"
        ]
    );
    assert_eq!(all[before].payload["label"], "work");
    assert_eq!(all[before].payload["host"], "https://api.github.com");

    // Nothing is bound under that id any more, and trying is not recorded.
    let after = records(&runtime).await.len();
    assert!(runtime.unbind_integration(id).await.is_err());
    assert_eq!(records(&runtime).await.len(), after);
    assert!(runtime.verify_audit().await.unwrap().is_intact());
}

#[tokio::test]
async fn an_account_with_no_token_behind_it_is_listed_as_one_and_sends_nothing() {
    let (runtime, secrets, _guard) = runtime().await;
    // Loopback, so that if anything were sent the listener would see it; the
    // check must stop before the address is even considered.
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let host = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let id = bind(&runtime, "work", Some(&host), false).await;

    // Removed behind the account's back, as `agentos credential remove` would.
    secrets
        .delete(&network_key(&host, "work").unwrap())
        .unwrap();

    let listed = runtime.list_integrations().await.unwrap();
    assert!(!listed[0].credential_present);

    let check = runtime.test_integration(id).await.unwrap();
    assert_eq!(check.outcome, CheckOutcome::Unauthorised);
    assert!(
        check.detail.contains("no token is stored"),
        "{}",
        check.detail
    );
    assert!(
        listener.accept().is_err(),
        "a request was sent with no token"
    );
}

#[tokio::test]
async fn a_check_never_reaches_loopback_whatever_the_account_allows() {
    // A host on loopback is refused under the strict policy and under the
    // private-network one alike, before anything is sent: the operator's flag
    // widens to their own network, never to this machine.
    let (runtime, _secrets, _guard) = runtime().await;
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let host = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

    for (label, private_network) in [("strict", false), ("private", true)] {
        let id = bind(&runtime, label, Some(&host), private_network).await;
        let check = runtime.test_integration(id).await.unwrap();
        assert_eq!(check.outcome, CheckOutcome::WrongHost, "{label}");
        assert!(
            check.detail.contains("refused before connecting"),
            "{label}: {}",
            check.detail
        );
        assert!(!check.detail.contains("EXAMPLETOKEN"), "{}", check.detail);
    }
    assert!(listener.accept().is_err(), "loopback was connected to");
    // Refused before the token was asked for, so no spend is recorded: the
    // record is made when the token is released, as a run's is.
    assert!(
        !kinds(&records(&runtime).await).contains(&"network.credential.used"),
        "a spend was recorded for a request never made"
    );
}

#[tokio::test]
async fn the_registry_offers_github_whatever_is_bound_and_finds_accounts_as_they_are_bound() {
    let (runtime, _secrets, dir) = runtime().await;
    let registry = runtime.registry().clone();
    let names = |registry: &agentos_tools::ToolRegistry| {
        registry
            .all_metadata()
            .into_iter()
            .map(|metadata| metadata.name.clone())
            .collect::<Vec<_>>()
    };
    let before = names(&registry);
    for known in INTEGRATIONS {
        for tool in known.tools {
            assert!(
                before.iter().any(|name| name == tool),
                "{tool} is not registered"
            );
        }
    }

    // Nothing bound: the call fails cleanly and says what fixes it.
    let tool = registry.get("github.issues.list").unwrap();
    let arguments = tool
        .validate(&serde_json::json!({"repo": "acme/widgets"}))
        .unwrap();
    let refused = tool
        .plan(&arguments, &context(&dir))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("agentos integration add github"),
        "{refused}"
    );

    // Bound afterwards, through the operator path: the same registry, the
    // same tool, now acting as the account. Nothing was re-registered.
    bind(&runtime, "work", None, false).await;
    let plan = tool.plan(&arguments, &context(&dir)).await.unwrap();
    assert!(
        plan.affected_resources
            .iter()
            .any(|resource| resource.contains("work")),
        "{:?}",
        plan.affected_resources
    );
    bind(&runtime, "personal", None, false).await;
    assert!(Arc::ptr_eq(&registry, runtime.registry()));
    assert_eq!(names(runtime.registry()), before);

    // Two bound, and the call names neither: it is refused, not guessed.
    let refused = tool
        .plan(&arguments, &context(&dir))
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("personal, work"), "{refused}");
}

#[tokio::test]
async fn a_call_made_as_an_account_is_shown_as_its_last_use() {
    let (runtime, _secrets, _guard) = runtime().await;
    bind(&runtime, "work", None, false).await;
    assert!(
        runtime.list_integrations().await.unwrap()[0]
            .account
            .last_used_at
            .is_none()
    );

    let directory = SqliteAccountDirectory::new(runtime.database().clone());
    let account = directory.resolve("github", None).await.unwrap();
    directory.record_use(&account).await;
    assert!(
        runtime.list_integrations().await.unwrap()[0]
            .account
            .last_used_at
            .is_some()
    );
}

#[test]
fn listing_the_catalogue_opens_no_database() {
    let dir = TempDir::new().unwrap();
    let config = RuntimeConfig::rooted_at(dir.path());
    let registry = agentos_runtime::build_registry(&config);
    assert!(registry.get("github.pulls.merge").is_some());
    assert!(
        !config.database_path.exists(),
        "listing the tools created {}",
        config.database_path.display()
    );
}
