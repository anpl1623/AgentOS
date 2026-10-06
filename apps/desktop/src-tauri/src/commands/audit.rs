//! The audit chain: verifying it, checking its health, and opening records.

use agentos_core::ids::{EventId, TaskRunId};
use agentos_runtime::Runtime;
use tauri::State;
use tokio::sync::Mutex;

use super::{Answer, DesktopError, parse_id};
use crate::dto::{AuditHealth, AuditRecordView};
use crate::state::{AppState, AuditWatch};

/// Verify the whole audit chain, rehashing every record.
///
/// The deliberate check Settings offers. Its verdict also replaces the routine
/// health check's, so the dashboard cannot call intact a chain this has just
/// found broken, or the reverse.
#[tauri::command]
pub async fn verify_audit(state: State<'_, AppState>) -> Answer<Vec<String>> {
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
        checked_at: agentos_core::format_timestamp(&agentos_core::now()),
    })
}

/// Rehash the whole chain, and check the stretch since the checkpoint against
/// the checkpoint, reporting what either finds.
///
/// The whole-chain pass alone is not the stricter of the two. It proves the
/// log is consistent with itself, and a log rewritten consistently from some
/// record onwards is; only the checkpoint remembers what that record's hash
/// was when this process last proved it. So the deliberate check includes
/// the routine one, and its verdict, which replaces the watch's, can only
/// clear a break that neither of them can find any more.
pub(crate) async fn verify_whole_chain(
    runtime: &Runtime,
    watch: &Mutex<AuditWatch>,
) -> Answer<Vec<String>> {
    let mut watch = watch.lock().await;
    let whole = runtime.verify_audit().await?;
    let (since, verified) = runtime.verify_audit_from(Some(&watch.verified)).await?;
    watch.verified = verified;

    let mut breaks = whole.breaks;
    for found in since.breaks {
        if !breaks.contains(&found) {
            breaks.push(found);
        }
    }
    watch.intact = breaks.is_empty();
    Ok(breaks.iter().map(ToString::to_string).collect())
}
