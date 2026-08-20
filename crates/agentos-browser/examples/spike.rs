//! Manual smoke check: launch a browser, navigate, extract.
//!
//! Run with `cargo run -p agentos-browser --example spike -- <url>`.
//! Each step is time-boxed and prints as it completes, so a hang is attributed
//! to a step rather than to "the browser".
#![allow(clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use agentos_browser::{BrowserOptions, BrowserSession};
use agentos_core::ids::TaskRunId;

async fn step<T>(name: &str, seconds: u64, work: impl Future<Output = T>) -> T {
    let started = Instant::now();
    match tokio::time::timeout(Duration::from_secs(seconds), work).await {
        Ok(value) => {
            println!("  ok   {name} ({}ms)", started.elapsed().as_millis());
            value
        }
        Err(_) => {
            println!("  HUNG {name} (>{seconds}s)");
            std::process::exit(1);
        }
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "about:blank".to_owned());

    let found = agentos_browser::locate(None);
    println!("executable: {found:?}");
    let Some(_) = found else {
        println!("{}", agentos_browser::install_hint());
        return;
    };

    let options = BrowserOptions::new(std::env::temp_dir().join("agentos-spike"));
    let session = step(
        "launch",
        45,
        BrowserSession::launch(&options, TaskRunId::new()),
    )
    .await
    .expect("launch");

    let page = step("page handle", 10, session.page()).await;
    step("goto", 30, async {
        page.goto(&url).await.expect("goto");
    })
    .await;
    step("wait_for_navigation", 30, async {
        let _ = page.wait_for_navigation().await;
    })
    .await;

    let title = step("title", 10, async { page.get_title().await.ok().flatten() }).await;
    println!("  title: {title:?}");

    let text = step("innerText", 15, async {
        page.evaluate("document.body ? document.body.innerText : ''")
            .await
            .ok()
            .and_then(|value| value.into_value::<String>().ok())
    })
    .await;
    println!(
        "  text: {:?}",
        text.map(|t| t.chars().take(120).collect::<String>())
    );

    step("close", 20, session.close()).await;
    println!("done");
}
