//! A navigation ends on the origin it was authorised for, or the browser
//! leaves; and a page that moves itself later is not acted on.
//!
//! Two loopback servers on two ports are two origins. The first sends the
//! browser to the second, by an HTTP redirect, a meta refresh, a script run on
//! load or a timer; `browser.navigate` is authorised against the first, and
//! must not leave the agent working on the second.
//!
//! If no Chromium-family browser is installed the test announces that and
//! returns rather than failing. A skipped test that says so is honest; one that
//! quietly passes is not.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;

use agentos_browser::{BrowserOptions, BrowserPool};
use agentos_core::ids::{AgentId, TaskId, TaskRunId};
use agentos_core::permission::{Capability, ResourceRef};
use agentos_tools::{Tool, ToolContext, ToolError};
use tokio_util::sync::CancellationToken;

fn browser_available() -> bool {
    if agentos_browser::locate(None).is_some() {
        return true;
    }
    println!(
        "SKIPPED: no Chromium-family browser found, so the redirect test did not run.\n\
         Install Chrome or Chromium, or set {}, to exercise it.",
        agentos_browser::EXECUTABLE_ENV
    );
    false
}

/// Serve every request with `respond(path)` on a loopback port, forever.
///
/// Returns the origin. One thread, one connection at a time, no keep-alive:
/// enough for one browser loading one page.
fn serve(respond: impl Fn(&str) -> String + Send + 'static) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let origin = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }
            // Drain the headers so the browser is not reset mid-request.
            let mut header = String::new();
            while reader.read_line(&mut header).is_ok_and(|read| read > 2) {
                header.clear();
            }
            let path = request_line.split(' ').nth(1).unwrap_or("/").to_owned();
            let _ = stream.write_all(respond(&path).as_bytes());
        }
    });
    origin
}

fn page(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: \
         close\r\n\r\n{body}",
        body.len()
    )
}

fn redirect(location: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: \
         close\r\n\r\n"
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_redirect_to_another_origin_is_left_and_named() {
    if !browser_available() {
        return;
    }

    let elsewhere = serve(|_| page("<title>Elsewhere</title><p>not authorised</p>"));
    let target = elsewhere.clone();
    let authorised = serve(move |path| match path {
        "/away" => redirect(&format!("{target}/landed")),
        "/hop" => redirect("/home"),
        _ => page("<title>Home</title><p>authorised</p>"),
    });

    let profiles = std::env::temp_dir().join(format!("agentos-redirect-{}", std::process::id()));
    let pool = Arc::new(BrowserPool::new(BrowserOptions::new(&profiles)));
    let navigate = agentos_browser::tools::Navigate::new(pool.clone());
    let run_id = TaskRunId::new();
    let context = ToolContext::new(AgentId::new(), TaskId::new(), run_id, profiles.clone());

    // A redirect within the authorised origin is followed as before.
    let output = navigate
        .execute(
            serde_json::json!({"url": format!("{authorised}/hop")}),
            &context,
            CancellationToken::new(),
        )
        .await
        .expect("a same-origin redirect is still a navigation to that origin");
    assert_eq!(
        output.structured.as_ref().unwrap()["url"],
        format!("{authorised}/home")
    );

    // And the page it stayed on is read, by a call authorised for its origin.
    let read = agentos_browser::tools::Extract::new(pool.clone())
        .execute(
            serde_json::json!({}),
            &authorised_for(&context, "read", &authorised),
            CancellationToken::new(),
        )
        .await
        .expect("a page still on its authorised origin is read");
    assert!(
        read.content.body.contains("authorised"),
        "{}",
        read.content.body
    );

    // A redirect to another origin is refused after the fact, by name.
    let refused = navigate
        .execute(
            serde_json::json!({"url": format!("{authorised}/away")}),
            &context,
            CancellationToken::new(),
        )
        .await;
    let Err(ToolError::Failed(message)) = refused else {
        panic!("a cross-origin landing must fail, got {refused:?}");
    };
    assert!(
        message.contains(&elsewhere),
        "the error must name where the page landed: {message}"
    );
    assert!(message.contains("Navigate to"), "{message}");

    // And the browser is no longer on that page: nothing the agent reads,
    // types or captures next comes from an origin no decision covered.
    let session = pool.existing_session(run_id).await;
    let current = match &session {
        Some(session) => session.current_url().await,
        None => None,
    };
    assert_eq!(current, None, "the browser stayed on {current:?}");

    pool.close_all().await;
    let _ = std::fs::remove_dir_all(&profiles);
}

/// `context` as the pipeline would hand it to a call authorised for `action`
/// on `origin`.
fn authorised_for(context: &ToolContext, action: &str, origin: &str) -> ToolContext {
    let mut context = context.clone();
    context.authorised =
        vec![
            Capability::new("browser", action).with_resource(ResourceRef::Origin {
                origin: origin.to_owned(),
            }),
        ];
    context
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_that_moves_itself_as_it_loads_is_left_and_named() {
    if !browser_available() {
        return;
    }

    let elsewhere = serve(|_| page("<title>Elsewhere</title><p>not authorised</p>"));
    let target = elsewhere.clone();
    let authorised = serve(move |path| match path {
        "/meta" => page(&format!(
            "<meta http-equiv=refresh content='0;url={target}/landed'><title>m</title>"
        )),
        "/onload" => page(&format!(
            "<title>o</title><script>addEventListener('load', () => \
             {{ location = '{target}/landed'; }})</script>"
        )),
        _ => page("<title>Home</title>"),
    });

    let profiles = std::env::temp_dir().join(format!("agentos-onload-{}", std::process::id()));
    let pool = Arc::new(BrowserPool::new(BrowserOptions::new(&profiles)));
    let navigate = agentos_browser::tools::Navigate::new(pool.clone());
    let run_id = TaskRunId::new();
    let context = ToolContext::new(AgentId::new(), TaskId::new(), run_id, profiles.clone());

    for path in ["/meta", "/onload"] {
        let refused = navigate
            .execute(
                serde_json::json!({"url": format!("{authorised}{path}")}),
                &context,
                CancellationToken::new(),
            )
            .await;
        let Err(ToolError::Failed(message)) = refused else {
            panic!("{path}: a page that moved itself on load must fail, got {refused:?}");
        };
        assert!(message.contains(&elsewhere), "{path}: {message}");
        let current = match pool.existing_session(run_id).await {
            Some(session) => session.current_url().await,
            None => None,
        };
        assert_eq!(current, None, "{path}: the browser stayed on {current:?}");
    }

    pool.close_all().await;
    let _ = std::fs::remove_dir_all(&profiles);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_that_moves_itself_later_is_not_acted_on() {
    if !browser_available() {
        return;
    }

    let elsewhere = serve(|_| page("<title>Elsewhere</title><p>not authorised</p>"));
    let target = elsewhere.clone();
    // Long enough after load that the navigation has returned.
    let authorised = serve(move |_| {
        page(&format!(
            "<title>t</title><p>authorised</p><script>setTimeout(() => \
             {{ location = '{target}/landed'; }}, 2000)</script>"
        ))
    });

    let profiles = std::env::temp_dir().join(format!("agentos-timer-{}", std::process::id()));
    let pool = Arc::new(BrowserPool::new(BrowserOptions::new(&profiles)));
    let run_id = TaskRunId::new();
    let context = ToolContext::new(AgentId::new(), TaskId::new(), run_id, profiles.clone());

    agentos_browser::tools::Navigate::new(pool.clone())
        .execute(
            serde_json::json!({"url": format!("{authorised}/")}),
            &context,
            CancellationToken::new(),
        )
        .await
        .expect("the page is on its origin when the navigation returns");

    // The call below was authorised while the page was on its own origin;
    // by the time it runs, the page has moved.
    let session = pool.existing_session(run_id).await.unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !session
        .current_url()
        .await
        .is_some_and(|url| url.starts_with(&elsewhere))
    {
        assert!(std::time::Instant::now() < deadline, "the page never moved");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let refused = agentos_browser::tools::Extract::new(pool.clone())
        .execute(
            serde_json::json!({}),
            &authorised_for(&context, "read", &authorised),
            CancellationToken::new(),
        )
        .await;
    let Err(ToolError::Failed(message)) = refused else {
        panic!("a call authorised for another origin must not read this one, got {refused:?}");
    };
    assert!(message.contains(&elsewhere), "{message}");
    assert!(!message.contains("not authorised</p>"), "{message}");

    pool.close_all().await;
    let _ = std::fs::remove_dir_all(&profiles);
}
