//! Application state, the approval bridge, the live event stream and the
//! scheduler's supervisor.
//!
//! Everything here is plumbing between the runtime and the webview. No decision
//! about what an agent may do is made in this file, or anywhere else in this
//! crate — the interface renders approvals and forwards answers; the policy
//! engine decides, exactly as it does for the CLI.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use agentos_core::approval::ApprovalRequest;
use agentos_core::ids::ApprovalId;
use agentos_runtime::{
    AuditCheckpoint, Runtime, RuntimeError, Scheduler, SchedulerOptions, SchedulerTransition,
};
use agentos_tools::{ApprovalGate, ApprovalOutcome};
use async_trait::async_trait;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, UserAttentionType};
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;

use crate::commands::{Answer, DesktopError};
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

        announce_waiting(&self.app, self.bridge.waiting_ids().await.len(), true);

        let outcome = tokio::select! {
            () = cancel.cancelled() => ApprovalOutcome::Cancelled,
            answer = receiver => answer.unwrap_or(ApprovalOutcome::Cancelled),
        };

        self.bridge.forget(request.id).await;
        let _ = self.app.emit(APPROVAL_RESOLVED, &request.id.to_string());
        announce_waiting(&self.app, self.bridge.waiting_ids().await.len(), false);
        outcome
    }
}

/// The dock or taskbar badge for a number of waiting approvals.
///
/// No badge at zero, rather than a zero: a badge is a call to act, and one that
/// is always present stops being read.
#[must_use]
pub fn badge_count(waiting: usize) -> Option<i64> {
    (waiting > 0).then(|| i64::try_from(waiting).unwrap_or(i64::MAX))
}

/// Show how many approvals are waiting where a person can see it with the
/// window behind another.
///
/// The loop this application exists for stalls when nobody notices an agent is
/// waiting, and the in-window surfaces are invisible to someone working in
/// another application. So the badge always follows the count, and a new
/// request asks for attention when no window is in front. Both are best effort:
/// a platform without a badge (Windows has none) or a window manager that
/// ignores the request is not a reason to fail an approval, so failures are
/// logged and dropped. These are calls from Rust, which the webview's
/// capability grants do not govern.
fn announce_waiting(app: &AppHandle, waiting: usize, asking: bool) {
    for window in app.webview_windows().values() {
        if let Err(error) = window.set_badge_count(badge_count(waiting)) {
            tracing::debug!(%error, "could not set the window badge");
        }
        if asking
            && !window.is_focused().unwrap_or(false)
            && let Err(error) =
                window.request_user_attention(Some(UserAttentionType::Informational))
        {
            tracing::debug!(%error, "could not request attention for the window");
        }
    }
}

/// How far this process has verified the audit chain, and what it found.
///
/// Verifying the whole chain rehashes every record the log has ever held, so
/// the routine health check verifies only what was written since it last
/// looked, through [`Runtime::verify_audit_from`]; that call owns the anchoring
/// of each new stretch onto the last record proved. A record older than the
/// checkpoint is not rehashed again by this check; the full verification in
/// Settings remains the deliberate way to do that, and its answer is folded in
/// here so the two never disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditWatch {
    /// The last record proved to extend an intact chain.
    ///
    /// The runtime does not move it past a break, so a broken stretch is
    /// verified, and reported, again on every check until it is looked at.
    pub verified: AuditCheckpoint,
    /// Whether everything verified so far is intact.
    ///
    /// Sticky: a chain is only as trustworthy as its worst link, so a break
    /// stays reported however many good records follow it. Only a full
    /// verification sets it again.
    pub intact: bool,
}

impl Default for AuditWatch {
    fn default() -> Self {
        Self {
            verified: AuditCheckpoint::genesis(),
            intact: true,
        }
    }
}

/// Everything the commands need.
#[derive(Debug)]
pub struct AppState {
    /// The runtime. The desktop application is one of its clients.
    pub runtime: Runtime,
    /// Routes approval answers back to waiting runs.
    pub approvals: ApprovalBridge,
    /// How far the audit chain has been verified.
    ///
    /// Held across each check, so two checks arriving together verify the
    /// same records once rather than racing to move the checkpoint.
    pub audit: Mutex<AuditWatch>,
    /// Held while a retry decides whether a task may run again and starts it.
    ///
    /// Without it, two clicks on Retry can both see a failed attempt and both
    /// start one, interleaving two traces under a single objective.
    pub retrying: Mutex<()>,
    /// The scheduler, run inside the application. Constructed stopped.
    pub scheduler: Arc<SchedulerSupervisor>,
    /// Set once the window may close without asking: the operator confirmed,
    /// nothing was live, or the process is already exiting.
    pub closing: AtomicBool,
    /// Woken when the interface acknowledges a held close, which is how the
    /// guard knows somebody is there to be asked.
    pub close_acknowledged: tokio::sync::Notify,
}

impl AppState {
    /// Build the state around a runtime.
    #[must_use]
    pub fn new(runtime: Runtime) -> Self {
        Self {
            scheduler: Arc::new(SchedulerSupervisor::new(
                runtime.clone(),
                SchedulerOptions::default(),
            )),
            runtime,
            approvals: ApprovalBridge::new(),
            audit: Mutex::new(AuditWatch::default()),
            retrying: Mutex::new(()),
            closing: AtomicBool::new(false),
            close_acknowledged: tokio::sync::Notify::new(),
        }
    }
}

/// The refusal shown when another process holds the scheduler's lease.
pub const SCHEDULER_ELSEWHERE: &str = "the scheduler is already running in another process on \
     this installation, most likely `agentos schedule run` in a terminal. Schedules fire and \
     queued tasks start either way; stop that one first to run the scheduler here instead.";

/// The refusal shown when new pacing would stop scheduled runs in progress.
pub const SCHEDULER_BUSY: &str = "the scheduler has runs in progress, and changing how it is \
     paced restarts it, which would stop them. Nothing was changed. Try again once they have \
     finished, or turn the scheduler off, which stops them and says so.";

/// Turn a scheduler's failure into the words an operator is shown.
///
/// The lease refusal is the one an operator can act on, and the runtime's own
/// message for it does not say where the other scheduler probably is. Every
/// other failure passes through as the runtime wrote it.
#[must_use]
pub fn scheduler_failure(error: RuntimeError) -> DesktopError {
    match error {
        RuntimeError::SchedulerAlreadyRunning => {
            DesktopError::Rejected(SCHEDULER_ELSEWHERE.to_owned())
        }
        other => DesktopError::Runtime(other),
    }
}

/// Runs the scheduler inside the application, so unattended work happens
/// without a terminal left open.
///
/// The scheduler is driven behind the runtime's default `DenyAllGate`, and
/// never given [`Scheduler::with_approvals`]. Everything the policy permits
/// outright proceeds; everything that would ask a person is refused with a note
/// the model can plan around. An approval prompt arriving hours later, out of
/// the context that produced it, to whoever happens to be at the window, is not
/// a human in the loop, and a card that gets clicked through because it is the
/// fortieth of the night is the approval gate made decorative.
///
/// A [`Scheduler`] is built afresh on every start, because its cancellation
/// token is one-shot: once stopped, a scheduler stays stopped.
#[derive(Debug)]
pub struct SchedulerSupervisor {
    runtime: Runtime,
    options: Mutex<SchedulerOptions>,
    running: Mutex<Option<Running>>,
    /// Why the last scheduler ended without being asked to, until the next
    /// successful start.
    failure: Mutex<Option<String>>,
}

/// A scheduler that has been started, and the task driving it.
#[derive(Debug)]
struct Running {
    scheduler: Arc<Scheduler>,
    /// Resolves once the scheduler has stopped and its stop is recorded.
    task: JoinHandle<Result<(), RuntimeError>>,
    started_at: agentos_core::Timestamp,
}

/// A supervisor's state, read at one moment.
#[derive(Debug, Clone)]
pub struct SupervisorStatus {
    /// Whether a scheduler is ticking.
    pub running: bool,
    /// The pacing a start uses, or the running scheduler's.
    pub options: SchedulerOptions,
    /// When the running scheduler started.
    pub started_at: Option<agentos_core::Timestamp>,
    /// Why the last scheduler stopped on its own, if it did.
    pub error: Option<String>,
}

impl SchedulerSupervisor {
    /// A stopped supervisor that will start schedulers with `options`.
    #[must_use]
    pub fn new(runtime: Runtime, options: SchedulerOptions) -> Self {
        Self {
            runtime,
            options: Mutex::new(options),
            running: Mutex::new(None),
            failure: Mutex::new(None),
        }
    }

    /// Start a scheduler, unless one is already running.
    ///
    /// `on_launch` records that a saved preference started it as the
    /// application opened, rather than somebody asking for it then.
    ///
    /// # Errors
    ///
    /// [`DesktopError::Rejected`] in plain words when another process holds
    /// the scheduler lease, and the runtime's error when its start could not
    /// be recorded. Either way nothing has ticked.
    pub async fn start(&self, on_launch: bool) -> Answer<()> {
        let mut running = self.running.lock().await;
        if let Some(current) = running.as_ref()
            && !current.task.inner().is_finished()
        {
            return Ok(());
        }
        if let Some(ended) = running.take() {
            self.collect(ended).await;
        }

        let options = *self.options.lock().await;
        let scheduler = Arc::new(Scheduler::new(self.runtime.clone(), options));

        // The lease first, so a refusal is this call's error, which the switch
        // can show, rather than a task that dies a moment after the switch has
        // said "on"; and so the start is recorded only for the scheduler that
        // will run.
        if let Err(error) = scheduler.take_lease() {
            let error = scheduler_failure(error);
            *self.failure.lock().await = Some(error.to_string());
            return Err(error);
        }

        // Then the record, before the first tick, so the chain shows the
        // scheduler starting ahead of anything it starts. A start that cannot
        // be recorded does not stand: unattended work with no record that
        // anything was set to do it is the gap the operator record exists to
        // close. Nothing has ticked yet, and dropping the scheduler releases
        // its lease.
        if let Err(error) = self
            .runtime
            .record_scheduler_state(SchedulerTransition::Started { on_launch }, &options)
            .await
        {
            *self.failure.lock().await = Some(error.to_string());
            return Err(error.into());
        }

        let runtime = self.runtime.clone();
        let ticking = Arc::clone(&scheduler);
        let task = tauri::async_runtime::spawn(async move {
            // `run` keeps the lease taken above.
            let ended = ticking.run().await;
            // However it ended, by request or by failure, from here nothing
            // fires, and the chain says so.
            if let Err(error) = runtime
                .record_scheduler_state(SchedulerTransition::Stopped, &options)
                .await
            {
                tracing::error!(%error, "could not record that the scheduler stopped");
            }
            if let Err(error) = &ended {
                tracing::error!(%error, "the scheduler stopped on its own");
            }
            ended
        });
        let started = Running {
            scheduler,
            task,
            started_at: agentos_core::now(),
        };

        *self.failure.lock().await = None;
        *running = Some(started);
        Ok(())
    }

    /// Stop the scheduler: cancel it, wait for its runs to drain, and join it.
    ///
    /// The runs it started are given the same cancellation the operator's stop
    /// button uses, and waited for, so none is left marked as running.
    pub async fn stop(&self) {
        let Some(current) = self.running.lock().await.take() else {
            return;
        };
        current.scheduler.shutdown().await;
        self.collect(current).await;
    }

    /// Whether a scheduler is ticking.
    pub async fn is_running(&self) -> bool {
        self.running
            .lock()
            .await
            .as_ref()
            .is_some_and(|current| !current.task.inner().is_finished())
    }

    /// Take new pacing for the next start.
    ///
    /// Pacing equal to what is in force changes nothing, so saving the same
    /// settings twice, or turning on a scheduler that is already on, leaves it
    /// running untouched. A running scheduler with different pacing has to be
    /// replaced, since a scheduler's pacing is fixed when it is built; it is
    /// stopped here, and the caller starts its successor with
    /// [`Self::start`]. Stopping a scheduler cancels the runs it started, and a
    /// change of pacing is not a decision to stop work, so while any of them
    /// is still going the change is refused and nothing is touched.
    ///
    /// # Errors
    ///
    /// [`DesktopError::Rejected`] with [`SCHEDULER_BUSY`] while the running
    /// scheduler has runs in progress.
    pub async fn repace(&self, options: SchedulerOptions) -> Answer<()> {
        let mut running = self.running.lock().await;
        let live = running
            .as_ref()
            .filter(|current| !current.task.inner().is_finished());
        if let Some(current) = live
            && *current.scheduler.options() != options
        {
            if !current.scheduler.shutdown_if_idle().await {
                return Err(DesktopError::Rejected(SCHEDULER_BUSY.to_owned()));
            }
            if let Some(stopped) = running.take() {
                self.collect(stopped).await;
            }
        }
        *self.options.lock().await = options;
        Ok(())
    }

    /// The supervisor's state now.
    pub async fn status(&self) -> SupervisorStatus {
        let mut running = self.running.lock().await;
        // A scheduler that ended on its own is collected here, so its reason
        // is reported rather than the switch quietly reading "off".
        if running
            .as_ref()
            .is_some_and(|current| current.task.inner().is_finished())
            && let Some(ended) = running.take()
        {
            self.collect(ended).await;
        }

        SupervisorStatus {
            running: running.is_some(),
            options: match running.as_ref() {
                Some(current) => *current.scheduler.options(),
                None => *self.options.lock().await,
            },
            started_at: running.as_ref().map(|current| current.started_at),
            error: self.failure.lock().await.clone(),
        }
    }

    /// Join a scheduler's task, keeping the reason if it failed.
    async fn collect(&self, ended: Running) {
        let failure = match ended.task.await {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(scheduler_failure(error).to_string()),
            Err(error) => Some(format!("the scheduler's task failed: {error}")),
        };
        if failure.is_some() {
            *self.failure.lock().await = failure;
        }
    }
}

/// Start the scheduler as the application opens, if the operator left it on.
///
/// Spawned, so a slow first tick does not hold the window back. A failure,
/// most often another process holding the lease, is kept for the status view
/// to report; the preference is left as it was, so the next launch tries
/// again.
pub fn resume_scheduler(supervisor: Arc<SchedulerSupervisor>) {
    tauri::async_runtime::spawn(async move {
        let preference = match supervisor.runtime.scheduler_preference().await {
            Ok(preference) => preference,
            Err(error) => {
                tracing::error!(%error, "could not read the scheduler preference");
                return;
            }
        };
        if !preference.enabled {
            return;
        }
        if let Err(error) = supervisor.repace(preference.options()).await {
            tracing::error!(%error, "could not apply the saved scheduler pacing");
            return;
        }
        if let Err(error) = supervisor.start(true).await {
            tracing::warn!(%error, "the saved scheduler preference could not be honoured");
        }
    });
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
                .resolve(ApprovalId::new(), ApprovalOutcome::Approved { note: None })
                .await
        );
    }

    #[tokio::test]
    async fn an_answer_reaches_the_waiting_run() {
        let bridge = ApprovalBridge::new();
        let id = ApprovalId::new();
        let receiver = bridge.register(id).await;

        assert_eq!(bridge.waiting_ids().await, vec![id]);
        assert!(
            bridge
                .resolve(id, ApprovalOutcome::Approved { note: None })
                .await
        );
        assert_eq!(
            receiver.await.ok(),
            Some(ApprovalOutcome::Approved { note: None })
        );

        // And it is no longer waiting, so a second click is a no-op rather than
        // a second decision.
        assert!(bridge.waiting_ids().await.is_empty());
        assert!(
            !bridge
                .resolve(id, ApprovalOutcome::Approved { note: None })
                .await
        );
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

    /// Records of `kind` in the runtime's audit log, oldest first.
    async fn payloads_of(runtime: &Runtime, kind: &str) -> Vec<serde_json::Value> {
        runtime
            .database()
            .audit_sink()
            .all()
            .await
            .unwrap()
            .into_iter()
            .filter(|record| record.kind == kind)
            .map(|record| record.payload)
            .collect()
    }

    /// A runtime over a real data directory, so the scheduler lease is real.
    async fn runtime_in(directory: &std::path::Path) -> Runtime {
        Runtime::open_with_secrets(
            agentos_runtime::RuntimeConfig::rooted_at(directory),
            Arc::new(agentos_secrets::InMemorySecretStore::new()),
        )
        .await
        .unwrap()
    }

    // These drive the supervisor on Tauri's own runtime, as the application
    // does: it spawns there, and a test runtime of its own would be torn down
    // under the scheduler's task.
    #[test]
    fn the_supervisor_starts_stops_and_starts_again() {
        tauri::async_runtime::block_on(async {
            let directory = tempfile::TempDir::new().unwrap();
            let runtime = runtime_in(directory.path()).await;
            let options = SchedulerOptions::default().with_tick(std::time::Duration::from_secs(5));
            let supervisor = SchedulerSupervisor::new(runtime.clone(), options);
            assert!(!supervisor.is_running().await);

            supervisor.start(true).await.unwrap();
            assert!(supervisor.is_running().await);
            // A second start while running is not a second scheduler.
            supervisor.start(false).await.unwrap();
            let status = supervisor.status().await;
            assert!(status.running);
            assert!(status.started_at.is_some());
            assert_eq!(status.options.tick, std::time::Duration::from_secs(5));

            supervisor.stop().await;
            assert!(!supervisor.is_running().await);
            // A new scheduler, since a stopped one's cancellation cannot be
            // undone; the lease the first held has been let go.
            supervisor.start(false).await.unwrap();
            assert!(supervisor.is_running().await);
            supervisor.stop().await;
            supervisor.stop().await;

            let started = payloads_of(&runtime, "operator.scheduler.started").await;
            assert_eq!(started.len(), 2);
            assert_eq!(started[0]["on_launch"], true);
            assert_eq!(started[1]["on_launch"], false);
            assert_eq!(
                payloads_of(&runtime, "operator.scheduler.stopped")
                    .await
                    .len(),
                2
            );
            assert!(supervisor.status().await.error.is_none());
        });
    }

    #[test]
    fn a_held_lease_is_refused_in_plain_words_and_recorded_as_nothing() {
        tauri::async_runtime::block_on(async {
            let directory = tempfile::TempDir::new().unwrap();
            let terminal = SchedulerSupervisor::new(
                runtime_in(directory.path()).await,
                SchedulerOptions::default(),
            );
            let runtime = runtime_in(directory.path()).await;
            let window = SchedulerSupervisor::new(runtime.clone(), SchedulerOptions::default());

            terminal.start(false).await.unwrap();
            let refused = window.start(false).await.unwrap_err();
            assert_eq!(refused.to_string(), SCHEDULER_ELSEWHERE);
            assert!(!window.is_running().await);
            let status = window.status().await;
            assert!(!status.running);
            assert_eq!(status.error.as_deref(), Some(SCHEDULER_ELSEWHERE));
            // One start recorded, the one that took the lease.
            assert_eq!(
                payloads_of(&runtime, "operator.scheduler.started")
                    .await
                    .len(),
                1
            );

            terminal.stop().await;
            window.start(false).await.unwrap();
            assert!(window.status().await.error.is_none());
            window.stop().await;
        });
    }

    #[test]
    fn only_the_lease_refusal_is_reworded() {
        assert_eq!(
            scheduler_failure(RuntimeError::SchedulerAlreadyRunning).to_string(),
            SCHEDULER_ELSEWHERE
        );
        assert!(SCHEDULER_ELSEWHERE.contains("agentos schedule run"));
        assert_eq!(
            scheduler_failure(RuntimeError::Rejected("no".to_owned())).to_string(),
            "no"
        );
    }

    /// A model that answers only once the test lets it, so a run can be held
    /// in progress for as long as a test needs.
    #[derive(Debug)]
    struct Held {
        release: Arc<tokio::sync::Semaphore>,
    }

    #[async_trait]
    impl agentos_providers::ModelProvider for Held {
        fn id(&self) -> &str {
            agentos_providers::provider_ids::MOCK
        }

        fn capabilities(&self) -> agentos_providers::ProviderCapabilities {
            agentos_providers::ProviderCapabilities {
                tools: true,
                usage_reporting: true,
                vision: false,
            }
        }

        async fn complete(
            &self,
            request: agentos_providers::CompletionRequest,
            cancel: CancellationToken,
        ) -> Result<agentos_providers::CompletionResponse, agentos_providers::ProviderError>
        {
            tokio::select! {
                () = cancel.cancelled() => {
                    return Err(agentos_providers::ProviderError::Cancelled);
                }
                permit = self.release.acquire() => drop(permit),
            }
            agentos_providers::MockProvider::answering("Done.")
                .complete(request, cancel)
                .await
        }
    }

    /// A runtime whose runs wait for `release`, with one agent and one task
    /// queued for it.
    async fn held_runtime(
        directory: &std::path::Path,
        release: &Arc<tokio::sync::Semaphore>,
    ) -> (Runtime, agentos_core::ids::TaskId) {
        let mut runtime = runtime_in(directory).await;
        runtime.set_provider_factory(Arc::new(agentos_runtime::FixedProviderFactory::new(
            Arc::new(Held {
                release: Arc::clone(release),
            }),
        )));
        let agent = runtime
            .create_agent(
                "worker",
                "Do the work.",
                agentos_core::agent::ModelConfig::new("mock", "scripted"),
                vec![],
            )
            .await
            .unwrap();
        let task = runtime
            .create_task(agent.id, "Queued.", &[], None)
            .await
            .unwrap();
        (runtime, task.id)
    }

    async fn wait_for_status(
        runtime: &Runtime,
        task: agentos_core::ids::TaskId,
        status: agentos_core::task::TaskStatus,
    ) {
        for _ in 0..400 {
            if runtime.task(task).await.unwrap().status == status {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("the task never became {status:?}");
    }

    #[test]
    fn new_pacing_waits_for_scheduled_runs_and_the_same_pacing_disturbs_nothing() {
        tauri::async_runtime::block_on(async {
            use agentos_core::task::TaskStatus;

            let directory = tempfile::TempDir::new().unwrap();
            let release = Arc::new(tokio::sync::Semaphore::new(0));
            let (runtime, task) = held_runtime(directory.path(), &release).await;
            let options = SchedulerOptions::default();
            let supervisor = SchedulerSupervisor::new(runtime.clone(), options);

            // Pacing for a stopped scheduler is kept for its start.
            let slower = options.with_tick(std::time::Duration::from_secs(90));
            supervisor.repace(slower).await.unwrap();
            assert!(!supervisor.is_running().await);
            supervisor.repace(options).await.unwrap();

            supervisor.start(false).await.unwrap();
            wait_for_status(&runtime, task, TaskStatus::Running).await;

            // The same pacing again, as saving unchanged settings sends:
            // nothing is restarted and the run goes on.
            supervisor.repace(options).await.unwrap();
            assert!(supervisor.is_running().await);
            // New pacing would mean a restart, and a restart would cancel the
            // run, so it is refused and nothing changes.
            let refused = supervisor.repace(slower).await.unwrap_err();
            assert_eq!(refused.to_string(), SCHEDULER_BUSY);
            assert!(supervisor.is_running().await);
            assert_eq!(supervisor.status().await.options, options);
            assert_eq!(
                runtime.task(task).await.unwrap().status,
                TaskStatus::Running
            );

            // Once the run is over, the same change goes through.
            release.add_permits(1);
            wait_for_status(&runtime, task, TaskStatus::Succeeded).await;
            supervisor.repace(slower).await.unwrap();
            assert!(!supervisor.is_running().await);
            supervisor.start(false).await.unwrap();
            assert_eq!(supervisor.status().await.options, slower);
            supervisor.stop().await;

            let started = payloads_of(&runtime, "operator.scheduler.started").await;
            assert_eq!(started.len(), 2);
            assert_eq!(started[1]["tick_seconds"], 90);
            assert!(
                payloads_of(&runtime, "agent.task.cancelled")
                    .await
                    .is_empty()
            );
        });
    }

    #[test]
    fn a_scheduler_is_recorded_as_started_before_it_starts_anything() {
        tauri::async_runtime::block_on(async {
            let directory = tempfile::TempDir::new().unwrap();
            let release = Arc::new(tokio::sync::Semaphore::new(1));
            let (runtime, task) = held_runtime(directory.path(), &release).await;
            let supervisor = SchedulerSupervisor::new(runtime.clone(), SchedulerOptions::default());

            // With the chain refusing writes, the start cannot be recorded,
            // and the queued task must still be queued afterwards: nothing
            // ticked on the strength of a start the chain never heard of.
            sqlx::query(
                "CREATE TRIGGER refuse_records BEFORE INSERT ON audit_events
                 BEGIN SELECT RAISE(ABORT, 'records refused'); END",
            )
            .execute(runtime.database().pool())
            .await
            .unwrap();
            assert!(supervisor.start(false).await.is_err());
            assert!(!supervisor.is_running().await);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            assert_eq!(
                runtime.task(task).await.unwrap().status,
                agentos_core::task::TaskStatus::Pending
            );
            sqlx::query("DROP TRIGGER refuse_records")
                .execute(runtime.database().pool())
                .await
                .unwrap();

            // Recorded, the start precedes the first thing it starts, and the
            // failed attempt let its lease go.
            supervisor.start(false).await.unwrap();
            wait_for_status(&runtime, task, agentos_core::task::TaskStatus::Succeeded).await;
            supervisor.stop().await;
            let kinds: Vec<String> = runtime
                .database()
                .audit_sink()
                .all()
                .await
                .unwrap()
                .into_iter()
                .map(|record| record.kind)
                .collect();
            let started = kinds
                .iter()
                .position(|kind| kind == "operator.scheduler.started")
                .unwrap();
            let ran = kinds
                .iter()
                .position(|kind| kind == "agent.task.started")
                .unwrap();
            assert!(started < ran, "{kinds:?}");
        });
    }

    #[test]
    fn the_badge_disappears_at_zero_rather_than_reading_zero() {
        assert_eq!(badge_count(0), None);
        assert_eq!(badge_count(1), Some(1));
        assert_eq!(badge_count(12), Some(12));
    }
}
