//! The audit chain: verifying it, checking its health, and opening records.

use agentos_core::ids::{EventId, TaskRunId};
use agentos_runtime::Runtime;
use tauri::State;
use tokio::sync::Mutex;

use super::{Answer, DesktopError, parse_id};
use crate::dto::{AuditHealth, AuditRecordView, AuditVerificationView};
use crate::state::{AppState, AuditWatch};

/// Verify the whole audit chain, rehashing every record, and check that the
/// records it holds account for every run they name.
///
/// The deliberate check Settings offers. Its verdict on the chain also
/// replaces the routine health check's, so the dashboard cannot call intact a
/// chain this has just found broken, or the reverse.
#[tauri::command]
pub async fn verify_audit(state: State<'_, AppState>) -> Answer<AuditVerificationView> {
    verify_whole_chain(&state.runtime, &state.audit).await
}

/// Whether the audit chain still verifies, checking only what is new.
#[tauri::command]
pub async fn audit_health(state: State<'_, AppState>) -> Answer<AuditHealth> {
    check_audit_health(&state.runtime, &state.audit).await
}

/// One audit record, in full.
#[tauri::command]
pub async fn audit_record(state: State<'_, AppState>, event_id: String) -> Answer<AuditRecordView> {
    let id: EventId = parse_id("event", &event_id)?;
    state
        .runtime
        .database()
        .audit_sink()
        .find(id)
        .await?
        .map(|record| AuditRecordView::from(&record))
        .ok_or_else(|| DesktopError::Rejected("there is no audit record with that identity".into()))
}

/// Every audit record a run wrote, in chain order.
#[tauri::command]
pub async fn audit_for_run(
    state: State<'_, AppState>,
    run_id: String,
) -> Answer<Vec<AuditRecordView>> {
    let id: TaskRunId = parse_id("run", &run_id)?;
    Ok(state
        .runtime
        .database()
        .audit_sink()
        .for_run(id)
        .await?
        .iter()
        .map(AuditRecordView::from)
        .collect())
}

/// Verify the records written since the checkpoint and report the chain's
/// health.
///
/// The watch stays locked for the whole check, so concurrent callers queue
/// behind one another and each record is verified once.
pub(crate) async fn check_audit_health(
    runtime: &Runtime,
    watch: &Mutex<AuditWatch>,
) -> Answer<AuditHealth> {
    let mut watch = watch.lock().await;
    let (verification, verified) = runtime.verify_audit_from(Some(&watch.verified)).await?;
    watch.verified = verified;
    watch.intact &= verification.is_intact();

    Ok(AuditHealth {
        events: runtime.database().audit_sink().count().await?,
        intact: watch.intact,
        unrecorded: runtime.audit().unrecorded(),
        checked_at: agentos_core::format_timestamp(&agentos_core::now()),
    })
}

/// Rehash the whole chain, check the stretch since the checkpoint against the
/// checkpoint, and check the log's coverage, reporting what any of them finds.
///
/// The whole-chain pass alone is not the stricter of the two chain checks. It
/// proves the log is consistent with itself, and a log rewritten consistently
/// from some record onwards is; only the checkpoint remembers what that
/// record's hash was when this process last proved it. So the deliberate check
/// includes the routine one, and its verdict, which replaces the watch's, can
/// only clear a break that neither of them can find any more.
///
/// Coverage is the question neither chain check can answer: a log can verify
/// perfectly and still hold a tool call that never ended or a delegated run
/// that wrote nothing. It is read from the same records as the rehash, read
/// once, and is reported beside the breaks rather than folded into the watch,
/// whose verdict speaks for the chain alone and is what the dashboard polls.
pub(crate) async fn verify_whole_chain(
    runtime: &Runtime,
    watch: &Mutex<AuditWatch>,
) -> Answer<AuditVerificationView> {
    let mut watch = watch.lock().await;
    let records = runtime.database().audit_sink().all().await?;
    let whole = agentos_audit::verify_chain(&records);
    let coverage = agentos_audit::verify_log_coverage(&records);
    let (since, verified) = runtime.verify_audit_from(Some(&watch.verified)).await?;
    watch.verified = verified;

    let mut breaks = whole.breaks;
    for found in since.breaks {
        if !breaks.contains(&found) {
            breaks.push(found);
        }
    }
    watch.intact = breaks.is_empty();
    Ok(AuditVerificationView {
        breaks: breaks.iter().map(ToString::to_string).collect(),
        gaps: coverage.gaps.iter().map(ToString::to_string).collect(),
    })
}
