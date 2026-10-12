//! The transport on its own, against a server on loopback.
//!
//! `network.request`'s hostile tests drive the transport through one tool and
//! the whole pipeline. These drive it the way an integration does, with a
//! request it built itself, to show that the refusals belong to the transport
//! and not to the tool in front of it: a credential goes only to its own
//! origin, the headers the client owns cannot be set, an echoed token comes
//! back redacted, a redirect is an answer, and the operator's private-network
//! permission does not reach loopback. Every server here is on `127.0.0.1`,
//! reached through the test-only constructor.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(
    unreachable_pub,
    reason = "an integration test binary has no external surface"
)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_tools::egress::{
    AddressPolicy, CredentialRef, Egress, EgressRequest, Method, ResponseBody, USER_AGENT, Url,
};
use agentos_tools::{CredentialResolver, REDACTED_CREDENTIAL, Secret, ToolContext, ToolError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// The token the operator stored, long and distinctive.
const TOKEN: &str = "ghp_7Xq2LmN9pR4sT6vW8yZ0aB1cD3eF5gH7";

/// A server that answers one request per connection and keeps every request.
struct Server {
    address: SocketAddr,
    received: Arc<Mutex<Vec<String>>>,
}

impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&received);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut chunk = [0_u8; 4096];
                    while !String::from_utf8_lossy(&buffer).contains("\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                        }
                    }
                    let request = String::from_utf8_lossy(&buffer).into_owned();
                    let response = respond(&request);
                    log.lock().unwrap().push(request);
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Self { address, received }
    }

    fn origin(&self) -> String {
        format!("http://{}", self.address)
    }

    fn url(&self, path: &str) -> Url {
        Url::parse(&format!("{}{path}", self.origin())).unwrap()
    }

    fn received(&self) -> Vec<String> {
        self.received.lock().unwrap().clone()
    }
}

fn header(head: &str, name: &str) -> Option<String> {
    head.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

fn respond(request: &str) -> String {
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let echoed = format!(
        "{{\"message\":\"Bad credentials\",\"received\":\"{}\"}}",
        header(request, "authorization").unwrap_or_default()
    );
    match path {
        // As an error page usually is sent: no length, the body ending when
        // the connection does, so the transport reads it to its limit.
        "/echo-401" => until_close("401 Unauthorized", &echoed),
        "/echo-500" => until_close("500 Internal Server Error", &echoed),
        "/redirect" => reply(
            "302 Found",
            "text/plain",
            "Location: http://169.254.169.254/latest/meta-data/\r\n",
            "",
        ),
        "/cookie" => reply(
            "200 OK",
            "text/plain",
            "Set-Cookie: session=c00kie\r\nX-Kept: yes\r\n",
            "ok",
        ),
        "/diff" => reply(
            "200 OK",
            "application/vnd.github.v3.diff; charset=utf-8",
            "",
            "diff --git a/x b/x\n",
        ),
        // A long diff, as GitHub sends one: its length declared, and the
        // presented credential written across the point a 100-byte limit
        // cuts, by a server that quotes what it was sent.
        "/long-diff" => reply(
            "200 OK",
            "application/vnd.github.v3.diff",
            "",
            &format!(
                "{}{}\n{}",
                "+".repeat(90),
                header(request, "authorization").unwrap_or_default(),
                "+line\n".repeat(4096)
            ),
        ),
        "/long-text" => reply("200 OK", "text/plain", "", &"x".repeat(4096)),
        _ => reply("200 OK", "text/plain", "", "ok"),
    }
}

fn until_close(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}"
    )
}

fn reply(status: &str, content_type: &str, extra: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Connection: close\r\n{extra}\r\n{body}",
        body.len()
    )
}

/// A store holding one credential, counting every lookup.
#[derive(Debug)]
struct Store {
    origin: String,
    name: String,
    lookups: AtomicUsize,
}

impl Store {
    fn holding(origin: &str, name: &str) -> Arc<Self> {
        Arc::new(Self {
            origin: origin.to_owned(),
            name: name.to_owned(),
            lookups: AtomicUsize::new(0),
        })
    }

    fn lookups(&self) -> usize {
        self.lookups.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl CredentialResolver for Store {
    async fn resolve(&self, origin: &str, name: &str) -> Option<Secret> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        (origin == self.origin && name == self.name).then(|| Secret::new(TOKEN))
    }
}

fn context(store: Arc<Store>) -> ToolContext {
    ToolContext::new(
        AgentId::new(),
        TaskId::new(),
        TaskRunId::new(),
        std::env::temp_dir(),
    )
    .with_credentials(store)
}

/// Whether any run of eight bytes of the token appears in `text`.
fn shows_token(text: &str) -> bool {
    TOKEN
        .as_bytes()
        .windows(8)
        .any(|piece| text.contains(std::str::from_utf8(piece).unwrap()))
}

fn testing() -> Egress {
    Egress::for_tests_admitting_loopback(AddressPolicy::Strict)
}

#[tokio::test]
async fn a_credential_is_never_sent_to_another_origin() {
    let home = Server::start().await;
    let elsewhere = Server::start().await;
    let store = Store::holding(&home.origin(), "work");

    // The account's credential, on a request to a different host: the pair a
    // confused or compromised caller would build.
    let request = EgressRequest::new(Method::GET, elsewhere.url("/user"))
        .with_credential(CredentialRef::new(home.origin(), "work"));
    let error = testing()
        .send(&context(Arc::clone(&store)), request)
        .await
        .unwrap_err();

    assert!(matches!(error, ToolError::Denied { .. }), "{error}");
    assert!(error.to_string().contains("bound to"), "{error}");
    assert_eq!(store.lookups(), 0, "the store was not even asked");
    assert!(elsewhere.received().is_empty());
    assert!(home.received().is_empty());
}

#[tokio::test]
async fn the_private_network_permission_does_not_reach_loopback() {
    let server = Server::start().await;
    let private = Egress::new(AddressPolicy::AllowPrivateNetwork);
    for url in [
        server.url("/"),
        Url::parse(&format!("http://localhost:{}/", server.address.port())).unwrap(),
        Url::parse(&format!("http://127.9.9.9:{}/", server.address.port())).unwrap(),
    ] {
        let error = private
            .send(
                &context(Store::holding("http://nowhere.test", "x")),
                EgressRequest::new(Method::GET, url.clone()),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ToolError::Denied { .. }), "{url}: {error}");
        assert!(error.to_string().contains("loopback"), "{url}: {error}");
    }
    assert!(server.received().is_empty());
}

#[tokio::test]
async fn an_echoed_token_comes_back_redacted_from_any_status() {
    let server = Server::start().await;
    let store = Store::holding(&server.origin(), "work");
    // The body opens with 48 bytes of JSON before the echoed token. A limit
    // of 55 falls seven bytes into it: cut there first and redacted after,
    // those seven are too short to recognise and reach the caller. Redacted
    // first, none of them can. A limit of 4096 keeps the whole body.
    for path in ["/echo-401", "/echo-500"] {
        for (cap, cut) in [(55, true), (4096, false)] {
            let request = EgressRequest::new(Method::GET, server.url(path))
                .with_credential(CredentialRef::new(server.origin(), "work"))
                .with_max_bytes(cap);
            let response = testing()
                .send(&context(Arc::clone(&store)), request)
                .await
                .unwrap();
            let body = response.text().unwrap();
            assert!(!shows_token(body), "{path} at {cap}: {body}");
            assert!(!body.contains(&TOKEN[..7]), "{path} at {cap}: {body}");
            assert!(!shows_token(&format!("{response:?}")), "{path} at {cap}");
            assert_eq!(response.truncated, cut, "{path} at {cap}");
            if !cut {
                assert!(body.contains(REDACTED_CREDENTIAL), "{path}: {body}");
            }
        }
    }

    // The server was sent the token, once per request, the only way it is
    // ever sent.
    let received = server.received();
    assert_eq!(received.len(), 4);
    for request in received {
        assert_eq!(
            header(&request, "authorization").as_deref(),
            Some(format!("Bearer {TOKEN}").as_str())
        );
        assert_eq!(header(&request, "user-agent").as_deref(), Some(USER_AGENT));
    }
}

#[tokio::test]
async fn a_redirect_is_the_answer_and_is_not_followed() {
    let server = Server::start().await;
    let response = testing()
        .send(
            &context(Store::holding(&server.origin(), "work")),
            EgressRequest::new(Method::GET, server.url("/redirect"))
                .with_credential(CredentialRef::new(server.origin(), "work")),
        )
        .await
        .unwrap();
    assert_eq!(response.status, 302);
    assert_eq!(response.reason, "Found");
    assert_eq!(
        response.header("LOCATION"),
        Some("http://169.254.169.254/latest/meta-data/")
    );
    assert_eq!(response.final_url, server.url("/redirect").to_string());
    assert_eq!(server.received().len(), 1, "exactly one request was made");
}

#[tokio::test]
async fn the_headers_the_transport_owns_cannot_be_set() {
    let server = Server::start().await;
    let store = Store::holding(&server.origin(), "work");
    for (name, value) in [
        ("Authorization", "Bearer other"),
        ("cookie", "a=b"),
        ("Host", "elsewhere.test"),
        ("Transfer-Encoding", "chunked"),
        ("X-HTTP-Method-Override", "DELETE"),
        ("x-a", "b\r\nx-b: c"),
    ] {
        let request = EgressRequest::new(Method::GET, server.url("/")).with_header(name, value);
        let error = testing()
            .send(&context(Arc::clone(&store)), request)
            .await
            .unwrap_err();
        assert!(matches!(error, ToolError::Failed(_)), "{name}: {error}");
    }
    // A slice of a response can carry a slice of the credential.
    for name in ["Range", "if-range"] {
        let request = EgressRequest::new(Method::GET, server.url("/"))
            .with_credential(CredentialRef::new(server.origin(), "work"))
            .with_header(name, "bytes=0-7");
        assert!(
            testing()
                .send(&context(Arc::clone(&store)), request)
                .await
                .is_err(),
            "{name}"
        );
    }
    assert!(server.received().is_empty());
    assert_eq!(store.lookups(), 0);
}

#[tokio::test]
async fn a_session_cookie_never_reaches_the_caller() {
    let server = Server::start().await;
    let response = testing()
        .send(
            &context(Store::holding("http://nowhere.test", "x")),
            EgressRequest::new(Method::GET, server.url("/cookie")),
        )
        .await
        .unwrap();
    assert_eq!(response.header("x-kept"), Some("yes"));
    assert_eq!(response.header("set-cookie"), None);
    assert!(!format!("{response:?}").contains("c00kie"));
}

#[tokio::test]
async fn a_diff_is_read_only_when_it_was_asked_for() {
    let server = Server::start().await;
    let store = Store::holding("http://nowhere.test", "x");
    let unasked = testing()
        .send(
            &context(Arc::clone(&store)),
            EgressRequest::new(Method::GET, server.url("/diff")),
        )
        .await
        .unwrap();
    assert!(
        matches!(unasked.body, ResponseBody::NotText { .. }),
        "{:?}",
        unasked.body
    );

    let asked = testing()
        .send(
            &context(store),
            EgressRequest::new(Method::GET, server.url("/diff")).accepting_diff(),
        )
        .await
        .unwrap();
    assert_eq!(asked.text(), Some("diff --git a/x b/x\n"));
    assert!(!asked.truncated);
}

#[tokio::test]
async fn a_long_diff_is_read_to_the_limit_and_redacted_before_the_cut() {
    let server = Server::start().await;
    let store = Store::holding(&server.origin(), "work");
    let request = || {
        EgressRequest::new(Method::GET, server.url("/long-diff"))
            .with_credential(CredentialRef::new(server.origin(), "work"))
            .with_max_bytes(100)
    };

    // A caller that did not ask for a diff is not handed one, whatever its
    // length.
    let refused = testing()
        .send(&context(Arc::clone(&store)), request())
        .await
        .unwrap();
    assert!(
        matches!(refused.body, ResponseBody::NotText { .. }),
        "{:?}",
        refused.body
    );

    // A caller that asked for a diff gets its first part, marked as cut,
    // and the credential written across the cut is redacted before it: no
    // piece of it is left on either side.
    let asked = testing()
        .send(&context(store), request().accepting_diff())
        .await
        .unwrap();
    let text = asked.text().expect("a diff over the limit is read");
    assert!(asked.truncated);
    assert_eq!(text.len(), 100, "{text}");
    assert!(text.starts_with(&"+".repeat(90)), "{text}");
    assert!(!shows_token(text), "{text}");
    assert!(
        text.ends_with(&REDACTED_CREDENTIAL[..text.len() - 97]),
        "{text}"
    );
    assert!(
        server.received()[1].contains(TOKEN),
        "the premise: it was sent"
    );
}

#[tokio::test]
async fn a_long_body_declared_over_the_limit_is_still_refused_without_a_diff() {
    let server = Server::start().await;
    let response = testing()
        .send(
            &context(Store::holding("http://nowhere.test", "x")),
            EgressRequest::new(Method::GET, server.url("/long-text")).with_max_bytes(100),
        )
        .await
        .unwrap();
    assert_eq!(
        response.body,
        ResponseBody::TooLarge {
            bytes: 4096,
            cap: 100
        }
    );
    assert!(response.truncated);
}
