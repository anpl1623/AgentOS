//! Network credentials: stored by the operator under one origin each, recorded
//! without their values, and released to a run only at that origin.
//!
//! The CLI and the desktop both store and remove credentials through these
//! methods, and every run resolves them through the resolver tested here, so
//! these are the runtime-level halves of what both clients promise.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use agentos_audit::AuditRecord;
use agentos_core::agent::ModelConfig;
use agentos_providers::{MockProvider, ScriptedTurn};
use agentos_runtime::{FixedProviderFactory, Runtime, RuntimeError, SecretStoreResolver};
use agentos_secrets::{InMemorySecretStore, SecretStore, is_credential_name, network_key};
use agentos_tools::network::{AddressPolicy, NetworkRequest};
use agentos_tools::{CredentialResolver, RecordingGate, Tool, ToolRegistry};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

const SECRET: &str = "sk-live-EXAMPLESECRET0123456789";

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

/// Everything the database holds, as text: the audit chain and every setting.
async fn everything_stored(runtime: &Runtime) -> String {
    let mut text = String::new();
    for record in records(runtime).await {
        text.push_str(&record.payload.to_string());
    }
    for (key, value) in runtime.database().settings().all().await.unwrap() {
        text.push_str(&key);
        text.push_str(&value);
    }
    text
}

#[tokio::test]
async fn a_credential_is_stored_under_its_canonical_origin_and_recorded_without_its_value() {
    let (runtime, secrets, _guard) = runtime().await;

    let bound = runtime
        .set_network_credential("HTTPS://API.Example.com:443/v1/ignored", "default", SECRET)
        .await
        .unwrap();
    assert_eq!(bound, "https://api.example.com");

    // The value is in the secret store, under a key that names the origin.
    let key = network_key("https://api.example.com", "default").unwrap();
    assert_eq!(secrets.get(&key).unwrap().expose(), SECRET);

    // The listing and the record name the origin and the credential.
    assert_eq!(
        runtime.list_network_credentials().await.unwrap(),
        vec![("https://api.example.com".to_owned(), "default".to_owned())]
    );
    let all = records(&runtime).await;
    let set: Vec<&AuditRecord> = all
        .iter()
        .filter(|record| record.kind == "operator.credential.set")
        .collect();
    assert_eq!(set.len(), 1);
    assert_eq!(set[0].payload["origin"], "https://api.example.com");
    assert_eq!(set[0].payload["name"], "default");

    // And nothing the database holds carries the value or any part of it.
    let stored = everything_stored(&runtime).await;
    assert!(!stored.contains("EXAMPLESECRET"), "{stored}");

    // Removal clears the store and the listing, and is recorded once.
    runtime
        .remove_network_credential("https://api.example.com", "default")
        .await
        .unwrap();
    assert!(secrets.get(&key).is_err());
    assert!(runtime.list_network_credentials().await.unwrap().is_empty());
    let removed = records(&runtime)
        .await
        .into_iter()
        .filter(|record| record.kind == "operator.credential.removed")
        .count();
    assert_eq!(removed, 1);
    assert!(runtime.verify_audit().await.unwrap().is_intact());
}

#[tokio::test]
async fn a_credential_that_cannot_be_bound_is_refused_and_not_recorded() {
    let (runtime, secrets, _guard) = runtime().await;
    let before = records(&runtime).await.len();

    for (origin, name, secret, says) in [
        ("file:///etc/passwd", "default", SECRET, "http"),
        (
            "https://user:pass@api.example.com",
            "default",
            SECRET,
            "credentials",
        ),
        // A dot in the name would let it carry part of a host into the key.
        (
            "https://api.example.com",
            "b.token",
            SECRET,
            "credential name",
        ),
        ("https://api.example.com", "", SECRET, "credential name"),
        ("https://api.example.com", "default", "   ", "no secret"),
        // A line break in a header value starts a header of its own.
        (
            "https://api.example.com",
            "default",
            "token\r\nX-Injected: 1",
            "control characters",
        ),
    ] {
        let refused = runtime.set_network_credential(origin, name, secret).await;
        let Err(RuntimeError::Rejected(message)) = refused else {
            panic!("{origin} / {name} was not refused: {refused:?}");
        };
        assert!(
            message.contains(says),
            "{origin} / {name}: `{message}` does not say `{says}`"
        );
    }

    // A refused name is not quoted back: it may be the secret, pasted into
    // the wrong field.
    let refused = runtime
        .set_network_credential("https://api.example.com", "sk.live.pasted", SECRET)
        .await
        .unwrap_err()
        .to_string();
    assert!(!refused.contains("pasted"), "{refused}");

    assert_eq!(records(&runtime).await.len(), before);
    assert!(runtime.list_network_credentials().await.unwrap().is_empty());
    assert!(
        secrets
            .get(&network_key("https://api.example.com", "default").unwrap())
            .is_err()
    );
}

#[test]
fn a_request_names_only_what_the_store_can_hold() {
    // The tool and the store keep separate copies of the name rule, because the
    // tools crate does not depend on the secret store. Were they to drift, a
    // request could be priced, put to the operator and approved for a name that
    // can never resolve, or refused a name the operator was allowed to store.
    let tool = NetworkRequest::new();
    let long = "a".repeat(64);
    let too_long = "a".repeat(65);
    for name in [
        "default",
        "api-token",
        "read_only",
        "v2",
        long.as_str(),
        too_long.as_str(),
        "",
        "b.token",
        "a/b",
        "a*",
        "a b",
        "a:b",
        "ä",
        "a\nb",
        "{x}",
    ] {
        let arguments = serde_json::json!({"url": "https://a.example/", "credential": name});
        assert_eq!(
            tool.validate(&arguments).is_ok(),
            is_credential_name(name),
            "the tool and the store disagree about `{}`",
            name.escape_debug()
        );
    }
}

#[tokio::test]
async fn a_credential_resolves_only_at_the_origin_it_was_stored_for() {
    let (runtime, secrets, _guard) = runtime().await;
    runtime
        .set_network_credential("https://crm.example.com", "default", SECRET)
        .await
        .unwrap();
    let resolver = SecretStoreResolver::new(secrets);

    let released = resolver
        .resolve("https://crm.example.com", "default")
        .await
        .expect("the credential is released at its own origin");
    assert_eq!(released.expose(), SECRET);

    // A hijacked run can ask for `default` anywhere. It is not there.
    for (origin, name) in [
        ("https://evil.example", "default"),
        ("http://crm.example.com", "default"),
        ("https://crm.example.com:8443", "default"),
        ("https://crm.example.com", "other"),
        // Spellings no normaliser produces are refused rather than looked up.
        ("https://CRM.example.com", "default"),
        ("https://crm.example.com/", "default"),
        ("https://crm.example.com:443", "default"),
        // The boundary between origin and name cannot be moved by the name.
        ("https://crm.example", "com.default"),
    ] {
        assert!(
            resolver.resolve(origin, name).await.is_none(),
            "`{name}` was released at {origin}"
        );
    }
}

#[tokio::test]
async fn storing_under_a_name_again_replaces_the_value_and_lists_it_once() {
    let (runtime, secrets, _guard) = runtime().await;
    runtime
        .set_network_credential("https://api.example.com", "default", "first-value")
        .await
        .unwrap();
    runtime
        .set_network_credential("https://api.example.com", "default", "second-value")
        .await
        .unwrap();
    runtime
        .set_network_credential("https://api.example.com", "admin", "third-value")
        .await
        .unwrap();

    assert_eq!(
        runtime.list_network_credentials().await.unwrap(),
        vec![
            ("https://api.example.com".to_owned(), "admin".to_owned()),
            ("https://api.example.com".to_owned(), "default".to_owned()),
        ]
    );
    let key = network_key("https://api.example.com", "default").unwrap();
    assert_eq!(secrets.get(&key).unwrap().expose(), "second-value");
}

/// A loopback server that answers every request `401`, echoing back the
/// `Authorization` header it was sent, as an integration's error page does.
///
/// Returns its origin and every request head it received.
fn echoing_server() -> (String, Arc<Mutex<Vec<String>>>) {
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
            let body = format!("{{\"error\":\"invalid token\",\"received\":\"{authorization}\"}}");
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
    });
    (origin, seen)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_spends_a_credential_only_where_it_is_bound_and_never_sees_it() {
    // Two servers on two ports are two origins. The credential is stored for
    // the first. The policy lets a run spend credentials at any loopback port,
    // so what keeps it from the second is the binding, not the policy.
    let (bound, bound_seen) = echoing_server();
    let (other, other_seen) = echoing_server();

    let (mut runtime, _secrets, _guard) = runtime().await;
    // Only a test builds the tool able to reach loopback; the runtime's own
    // registry cannot.
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(NetworkRequest::with_address_policy(
        AddressPolicy::PublicAndLoopback,
    )));
    runtime.set_registry(Arc::new(registry));

    let agent = runtime
        .create_agent(
            "integrations",
            "Call the API.",
            ModelConfig::new("mock", "scripted"),
            vec!["network.request".into()],
        )
        .await
        .unwrap();
    runtime
        .set_policy(
            agent.id,
            "default: deny\n\
             max_risk: high\n\
             permissions:\n  \
               network:\n    \
                 fetch: [\"http://127.0.0.1:*\"]\n    \
                 credential:\n      \
                   effect: allow\n      \
                   names: [\"http://127.0.0.1:*/*\"]\n",
        )
        .await
        .unwrap();
    runtime
        .set_network_credential(&bound, "default", SECRET)
        .await
        .unwrap();

    let provider = Arc::new(MockProvider::new(vec![
        ScriptedTurn::call(
            "c1",
            "network.request",
            serde_json::json!({"url": format!("{bound}/v1/me"), "credential": "default"}),
        ),
        ScriptedTurn::call(
            "c2",
            "network.request",
            serde_json::json!({"url": format!("{other}/v1/me"), "credential": "default"}),
        ),
        ScriptedTurn::text("Done."),
    ]));
    runtime.set_provider_factory(Arc::new(FixedProviderFactory::new(provider.clone())));

    let outcome = runtime
        .run_objective(
            agent.id,
            "Check who we are.",
            Arc::new(RecordingGate::approving()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(outcome.tainted, "a response body is untrusted data");

    // The bound origin was sent the credential; the other was sent nothing.
    let bound_heads = bound_seen.lock().unwrap().join("\n");
    assert!(
        bound_heads.contains(&format!("Bearer {SECRET}")),
        "the run's context carried no credentials: {bound_heads}"
    );
    let other_heads = other_seen.lock().unwrap().join("\n");
    assert!(!other_heads.contains(SECRET), "{other_heads}");

    // The spend is on the record, by origin and name.
    let used: Vec<AuditRecord> = records(&runtime)
        .await
        .into_iter()
        .filter(|record| record.kind == "network.credential.used")
        .collect();
    assert_eq!(used.len(), 1, "one credential was released, to one call");
    assert_eq!(used[0].payload["origin"], bound);
    assert_eq!(used[0].payload["name"], "default");

    // The 401 echoed the token. Neither the model, the audit chain nor the
    // stored trace holds it.
    let seen_by_model = format!("{:?}", provider.requests());
    assert!(!seen_by_model.contains("EXAMPLESECRET"), "{seen_by_model}");
    assert!(
        seen_by_model.contains("[redacted credential]"),
        "{seen_by_model}"
    );
    let stored = everything_stored(&runtime).await;
    assert!(!stored.contains("EXAMPLESECRET"), "{stored}");
    let trace = format!("{:?}", runtime.trace(outcome.run_id).await.unwrap());
    assert!(!trace.contains("EXAMPLESECRET"), "{trace}");
    assert!(runtime.verify_audit().await.unwrap().is_intact());
}
