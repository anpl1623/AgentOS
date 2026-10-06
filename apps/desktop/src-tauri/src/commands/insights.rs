//! What each tool has been doing.

use std::collections::BTreeMap;
use std::time::Duration;

use agentos_runtime::Runtime;
use tauri::State;

use super::Answer;
use crate::dto::ToolUsageView;
use crate::state::AppState;

/// The longest window [`tool_usage`] will look back over, in days.
const MAX_DAYS: i64 = 3_650;

/// How each tool has been used over the last `days` days, 7 unless given.
///
/// Every tool in the catalogue is listed, those with no calls included: a tool
/// an agent holds and never uses is worth seeing too.
#[tauri::command]
pub async fn tool_usage(
    state: State<'_, AppState>,
    days: Option<i64>,
) -> Answer<Vec<ToolUsageView>> {
    tool_usage_over(&state.runtime, days.unwrap_or(7)).await
}

/// Tool usage over the last `days` days, joined to the catalogue and ordered
/// by how many calls each tool took, busiest first.
pub(crate) async fn tool_usage_over(runtime: &Runtime, days: i64) -> Answer<Vec<ToolUsageView>> {
    let days = u64::try_from(days.clamp(1, MAX_DAYS)).unwrap_or(7);
    let since = agentos_core::now() - Duration::from_secs(days * 24 * 60 * 60);

    let mut rows: BTreeMap<String, ToolUsageView> = runtime
        .registry()
        .all_metadata()
        .iter()
        .map(|metadata| {
            (
                metadata.name.clone(),
                ToolUsageView {
                    tool: metadata.name.clone(),
                    calls: 0,
                    executed: 0,
                    denied: 0,
                    approval_denied: 0,
                    failed: 0,
                    under_taint: 0,
                    approvals: 0,
                    total_duration_ms: 0,
                    last_used_at: None,
                    risk: Some(metadata.risk.as_str().to_owned()),
                    returns_untrusted_data: metadata.returns_untrusted_data,
                },
            )
        })
        .collect();

    for usage in runtime.database().executions().usage_since(since).await? {
        let row = rows
            .entry(usage.tool.clone())
            .or_insert_with(|| ToolUsageView {
                tool: usage.tool.clone(),
                calls: 0,
                executed: 0,
                denied: 0,
                approval_denied: 0,
                failed: 0,
                under_taint: 0,
                approvals: 0,
                total_duration_ms: 0,
                last_used_at: None,
                risk: None,
                returns_untrusted_data: false,
            });
        row.calls = usage.calls;
        row.executed = usage.executed;
        row.denied = usage.denied;
        row.approval_denied = usage.approval_denied;
        row.failed = usage.failed;
        row.under_taint = usage.under_taint;
        row.approvals = usage.approvals;
        row.total_duration_ms = usage.total_duration_ms;
        row.last_used_at = Some(agentos_core::format_timestamp(&usage.last_used_at));
    }

    let mut out: Vec<ToolUsageView> = rows.into_values().collect();
    // Stable, so tools with equal counts stay in name order.
    out.sort_by_key(|row| std::cmp::Reverse(row.calls));
    Ok(out)
}
