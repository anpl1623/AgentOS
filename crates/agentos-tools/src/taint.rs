//! Taint tracking.
//!
//! A run that has read a webpage, a file or command output may be acting on text
//! an attacker wrote. The model cannot be relied on to notice, and asking it to
//! is not a control.
//!
//! So the runtime tracks it instead. Once a run ingests externally-influenced
//! data the run is *tainted*, and the policy engine raises the bar for
//! everything that follows: actions that would have run silently now need a
//! human. Escalation only ever tightens — a tainted run can never do more than
//! a clean one.
//!
//! This is what makes the classic "read a poisoned page, then exfiltrate"
//! sequence loud instead of silent. The exfiltration step is not blocked because
//! the runtime recognised it as malicious; it is surfaced because the run had
//! read something from outside and the action was consequential.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use agentos_core::trust::DataSource;

/// Tracks whether a run has ingested untrusted data.
#[derive(Debug, Default)]
pub struct TaintTracker {
    tainted: AtomicBool,
    sources: Mutex<Vec<DataSource>>,
}

impl TaintTracker {
    /// A clean tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A tracker that starts tainted, for a run resumed after ingesting data.
    #[must_use]
    pub fn already_tainted() -> Self {
        let tracker = Self::new();
        tracker.tainted.store(true, Ordering::SeqCst);
        tracker
    }

    /// Whether the run is tainted.
    #[must_use]
    pub fn is_tainted(&self) -> bool {
        self.tainted.load(Ordering::SeqCst)
    }

    /// Note that data from `source` entered the run.
    ///
    /// Returns `true` if this call is what flipped the run to tainted, so the
    /// caller can emit the transition event exactly once.
    pub fn observe(&self, source: &DataSource) -> bool {
        if !source.is_externally_influenced() {
            return false;
        }

        if let Ok(mut sources) = self.sources.lock()
            && !sources.contains(source)
        {
            sources.push(source.clone());
        }

        // `swap` rather than load-then-store: two tools finishing at once must
        // not both report that they were the one to raise the flag.
        !self.tainted.swap(true, Ordering::SeqCst)
    }

    /// Every distinct source that has contributed untrusted data.
    ///
    /// Shown on approval cards so a human can see what the agent has been
    /// reading before they decide.
    #[must_use]
    pub fn sources(&self) -> Vec<DataSource> {
        self.sources
            .lock()
            .map(|sources| sources.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn starts_clean() {
        let tracker = TaintTracker::new();
        assert!(!tracker.is_tainted());
        assert!(tracker.sources().is_empty());
    }

    #[test]
    fn operator_input_does_not_taint() {
        let tracker = TaintTracker::new();
        assert!(!tracker.observe(&DataSource::User));
        assert!(!tracker.observe(&DataSource::Runtime));
        assert!(!tracker.is_tainted());
    }

    #[test]
    fn external_data_taints_once() {
        let tracker = TaintTracker::new();
        let source = DataSource::Web {
            url: "https://example.com".into(),
        };

        assert!(tracker.observe(&source), "first observation should flip");
        assert!(tracker.is_tainted());
        assert!(
            !tracker.observe(&source),
            "second observation must not report a flip"
        );
    }

    #[test]
    fn distinct_sources_are_recorded_without_duplicates() {
        let tracker = TaintTracker::new();
        let web = DataSource::Web {
            url: "https://a".into(),
        };
        let file = DataSource::File {
            path: "/tmp/x".into(),
        };
        tracker.observe(&web);
        tracker.observe(&web);
        tracker.observe(&file);

        assert_eq!(tracker.sources(), vec![web, file]);
    }

    #[test]
    fn taint_is_never_cleared() {
        let tracker = TaintTracker::new();
        tracker.observe(&DataSource::File {
            path: "/tmp/x".into(),
        });
        tracker.observe(&DataSource::User);
        assert!(
            tracker.is_tainted(),
            "clean input must not launder a tainted run"
        );
    }

    #[test]
    fn exactly_one_concurrent_observer_reports_the_flip() {
        let tracker = Arc::new(TaintTracker::new());
        let flips: Vec<bool> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let tracker = tracker.clone();
                    scope.spawn(move || {
                        tracker.observe(&DataSource::Web {
                            url: format!("https://{i}"),
                        })
                    })
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .collect()
        });

        assert_eq!(flips.iter().filter(|flipped| **flipped).count(), 1);
        assert!(tracker.is_tainted());
        assert_eq!(tracker.sources().len(), 8);
    }
}
