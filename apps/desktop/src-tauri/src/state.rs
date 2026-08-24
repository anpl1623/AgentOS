//! Application state, the approval bridge and the live event stream.
//!
//! Everything here is plumbing between the runtime and the webview. No decision
//! about what an agent may do is made in this file, or anywhere else in this
//! crate — the interface renders approvals and forwards answers; the policy
//! engine decides, exactly as it does for the CLI.

use std::collections::HashMap;
use std::sync::Arc;

use agentos_core::approval::ApprovalRequest;
use agentos_core::ids::ApprovalId;
use agentos_runtime::Runtime;
use agentos_tools::{ApprovalGate, ApprovalOutcome};
use async_trait::async_trait;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;

use crate::dto::{ApprovalView, EventView, summarise_event};

/// Event emitted when an agent needs a human decision.
pub const APPROVAL_REQUESTED: &str = "agentos://approval-requested";

/// Event emitted once an approval has been answered, so every window agrees.
pub const APPROVAL_RESOLVED: &str = "agentos://approval-resolved";

/// Event emitted for every audit record, as it happens.
pub const ACTIVITY: &str = "agentos://activity";

/// Routes approval answers from the interface back to the run that is waiting.
///
/// A waiting run holds a [`oneshot::Sender`] here, keyed by request. The
/// interface answers by identifier, which is the only thing it needs to know —
/// it cannot reach the run, the gate, or the policy that produced the question.
#[derive(Debug, Clone, Default)]
pub struct ApprovalBridge {
    waiting: Arc<Mutex<HashMap<ApprovalId, oneshot::Sender<ApprovalOutcome>>>>,
}

impl ApprovalBridge {
    /// An empty bridge.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer a pending request.
    ///
    /// Returns `false` if nothing was waiting — the run finished, was cancelled,
    /// or this process did not raise the request. The interface treats that as
    /// "already resolved" rather than an error, because by the time a human
    /// clicks, either is possible.
    pub async fn resolve(&self, id: ApprovalId, outcome: ApprovalOutcome) -> bool {
        let sender = self.waiting.lock().await.remove(&id);
        match sender {
            Some(sender) => sender.send(outcome).is_ok(),
            None => false,
        }
    }

    /// Identifiers currently waiting on a human.
    pub async fn waiting_ids(&self) -> Vec<ApprovalId> {
        self.waiting.lock().await.keys().copied().collect()
    }

    async fn register(&self, id: ApprovalId) -> oneshot::Receiver<ApprovalOutcome> {
        let (sender, receiver) = oneshot::channel();
        self.waiting.lock().await.insert(id, sender);
        receiver
    }

    async fn forget(&self, id: ApprovalId) {
        self.waiting.lock().await.remove(&id);
    }
}

/// The gate a desktop run is given.
///
/// Pushes the request to the interface and waits. Cancelling the run stops the
/// wait — an operator must never have to answer a prompt for work they have
/// already stopped.
#[derive(Debug)]
pub struct DesktopApprovalGate {
    app: AppHandle,
    bridge: ApprovalBridge,
    /// The objective the run is pursuing, shown on the card for context.
    objective: String,
}

impl DesktopApprovalGate {
    /// Build a gate for one run.
    #[must_use]
    pub const fn new(app: AppHandle, bridge: ApprovalBridge, objective: String) -> Self {
        Self {
            app,
            bridge,
            objective,
        }
    }
}

#[async_trait]
impl ApprovalGate for DesktopApprovalGate {
    async fn request(
        &self,
        request: &ApprovalRequest,
        cancel: CancellationToken,
    ) -> ApprovalOutcome {
        let receiver = self.bridge.register(request.id).await;
        let view = ApprovalView::new(request, self.objective.clone());

        if let Err(error) = self.app.emit(APPROVAL_REQUESTED, &view) {
            // With no interface listening there is nobody to approve, and
            // proceeding would mean acting without the approval the policy
            // asked for.
            tracing::error!(%error, "could not deliver an approval request to the interface");
            self.bridge.forget(request.id).await;
            return ApprovalOutcome::Denied {
                note: Some("the approval request could not be shown".to_owned()),
            };
        }

        let outcome = tokio::select! {
            () = cancel.cancelled() => ApprovalOutcome::Cancelled,
            answer = receiver => answer.unwrap_or(ApprovalOutcome::Cancelled),
        };

        self.bridge.forget(request.id).await;
        let _ = self.app.emit(APPROVAL_RESOLVED, &request.id.to_string());
        outcome
    }
}

/// Everything the commands need.
#[derive(Debug)]
pub struct AppState {
    /// The runtime. The desktop application is one of its clients.
    pub runtime: Runtime,
    /// Routes approval answers back to waiting runs.
    pub approvals: ApprovalBridge,
}

impl AppState {
    /// Build the state around a runtime.
    #[must_use]
    pub fn new(runtime: Runtime) -> Self {
        Self {
            runtime,
            approvals: ApprovalBridge::new(),
        }
    }
}

/// Forward every audit event to the interface as it happens.
///
/// The durable log is still the source of truth; this is the live view. A
/// subscriber that falls behind loses events from the feed and none from the
/// log, which is the right way round.
pub fn stream_activity(app: AppHandle, runtime: &Runtime) {
    let mut events = runtime.audit().subscribe();
    // Tauri's runtime, not `tokio::spawn`. This is called from the setup hook,
    // which runs on the main thread before any reactor is entered, so
    // `tokio::spawn` aborts the process on a panic it cannot unwind.
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let payload = serde_json::to_value(&event.payload).unwrap_or_default();
                    let view = EventView {
                        id: event.id.to_string(),
                        sequence: None,
                        at: agentos_core::format_timestamp(&event.at),
                        kind: event.kind().to_owned(),
                        run_id: event.run_id.map(|id| id.to_string()),
                        task_id: event.task_id.map(|id| id.to_string()),
                        summary: summarise_event(&payload),
                        security_relevant: event.payload.is_security_relevant(),
                    };
                    if app.emit(ACTIVITY, &view).is_err() {
                        // The window has gone; nothing left to stream to.
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "activity feed fell behind");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolving_an_unknown_approval_reports_that_nothing_was_waiting() {
        let bridge = ApprovalBridge::new();
        assert!(
            !bridge
                .resolve(ApprovalId::new(), ApprovalOutcome::Approved)
                .await
        );
    }

    #[tokio::test]
    async fn an_answer_reaches_the_waiting_run() {
        let bridge = ApprovalBridge::new();
        let id = ApprovalId::new();
        let receiver = bridge.register(id).await;

        assert_eq!(bridge.waiting_ids().await, vec![id]);
        assert!(bridge.resolve(id, ApprovalOutcome::Approved).await);
        assert_eq!(receiver.await.ok(), Some(ApprovalOutcome::Approved));

        // And it is no longer waiting, so a second click is a no-op rather than
        // a second decision.
        assert!(bridge.waiting_ids().await.is_empty());
        assert!(!bridge.resolve(id, ApprovalOutcome::Approved).await);
    }

    #[tokio::test]
    async fn a_denial_carries_its_note() {
        let bridge = ApprovalBridge::new();
        let id = ApprovalId::new();
        let receiver = bridge.register(id).await;

        bridge
            .resolve(
                id,
                ApprovalOutcome::Denied {
                    note: Some("wrong recipient".to_owned()),
                },
            )
            .await;

        assert_eq!(
            receiver.await.ok(),
            Some(ApprovalOutcome::Denied {
                note: Some("wrong recipient".to_owned())
            })
        );
    }

    #[tokio::test]
    async fn forgetting_a_request_drops_the_waiter() {
        let bridge = ApprovalBridge::new();
        let id = ApprovalId::new();
        let receiver = bridge.register(id).await;
        bridge.forget(id).await;

        // The sender is gone, so the waiting run sees a closed channel and
        // treats it as a cancellation rather than hanging forever.
        assert!(receiver.await.is_err());
    }
}
