//! Browser automation for AgentOS.
//!
//! Deterministic and DOM-based, over the Chrome DevTools Protocol. Screenshots
//! exist for a human to look at and for a future vision fallback; they are not
//! how the agent decides where to click. `click on #send` is a reviewable
//! action in a way that `click at (412, 908)` is not, and it does not silently
//! start doing something different when a layout shifts.
//!
//! Kept separate from computer control on purpose. The two solve different
//! problems: the browser exposes structure, and a native application generally
//! does not.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod error;
pub mod locate;
pub mod session;
pub mod tools;

use std::sync::Arc;

pub use error::BrowserError;
pub use locate::{EXECUTABLE_ENV, install_hint, locate};
pub use session::{BrowserOptions, BrowserPool, BrowserSession};
pub use tools::all as browser_tools;

/// Build a pool and its tools together.
///
/// The pool is returned so the caller can close sessions at shutdown; the tools
/// already release per-run sessions through `Tool::end_run`.
#[must_use]
pub fn build(options: BrowserOptions) -> (Arc<BrowserPool>, Vec<Arc<dyn agentos_tools::Tool>>) {
    let pool = Arc::new(BrowserPool::new(options));
    let tools = tools::all(pool.clone());
    (pool, tools)
}

/// Names of every browser tool, for policy documents and `--tool` flags.
pub const TOOL_NAMES: &[&str] = &[
    "browser.navigate",
    "browser.click",
    "browser.type",
    "browser.extract",
    "browser.inspect",
    "browser.wait",
    "browser.back",
    "browser.forward",
    "browser.screenshot",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_advertised_tool_is_built() {
        let (_pool, tools) = build(BrowserOptions::new(std::env::temp_dir()));
        let names: Vec<String> = tools
            .iter()
            .map(|tool| tool.metadata().name.clone())
            .collect();
        assert_eq!(names.len(), TOOL_NAMES.len());
        for expected in TOOL_NAMES {
            assert!(
                names.iter().any(|name| name == expected),
                "missing {expected}"
            );
        }
    }

    #[test]
    fn browser_tools_are_marked_as_returning_untrusted_data() {
        // A page an agent reads is the canonical source of injected text. If any
        // of these stopped raising taint, the escalation would silently vanish.
        let (_pool, tools) = build(BrowserOptions::new(std::env::temp_dir()));
        for tool in &tools {
            let metadata = tool.metadata();
            if metadata.name == "browser.screenshot" {
                continue;
            }
            assert!(
                metadata.returns_untrusted_data,
                "`{}` reads the web and must raise taint",
                metadata.name
            );
        }
    }

    #[test]
    fn every_browser_tool_declares_a_capability() {
        let (_pool, tools) = build(BrowserOptions::new(std::env::temp_dir()));
        for tool in &tools {
            assert!(
                !tool.metadata().required_capabilities.is_empty(),
                "`{}` declares no capabilities",
                tool.metadata().name
            );
        }
    }
}
