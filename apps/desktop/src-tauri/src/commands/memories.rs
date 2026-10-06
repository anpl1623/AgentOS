//! What agents remember, read and written by their operator.
//!
//! Every run seeds its conversation from these rows, so the person responsible
//! for what an agent does has to be able to see them, correct them and remove
//! them. Writes go through the runtime's operator layer and are recorded.

use agentos_core::ids::{AgentId, MemoryId};
use agentos_core::memory::{MemoryKind, MemoryQuery};
use agentos_runtime::Runtime;
use tauri::State;

use super::{Answer, DesktopError, parse_id};
use crate::dto::{MemoryView, RememberInput, memory_kind};
use crate::state::AppState;

/// The confidence of a memory written without one: a person stating it.
const FULL_CONFIDENCE: f32 = 1.0;

/// An agent's memories, most recently revised first, optionally of one kind.
#[tauri::command]
pub async fn list_memories(
    state: State<'_, AppState>,
    agent_id: String,
    kind: Option<String>,
) -> Answer<Vec<MemoryView>> {
    let agent_id: AgentId = parse_id("agent", &agent_id)?;
    memories(&state.runtime, agent_id, kind.as_deref()).await
}

/// Record a memory for an agent, as written by a person.
///
/// The source is always [`agentos_core::trust::DataSource::User`], whatever the
/// interface might send; [`RememberInput`] has no field for one, and the
/// runtime's operator layer hard-codes it again. This command can attest only
/// that a human typed the text. A caller able to name the source could launder
/// a webpage's claim into a note that reads as the operator's own, and the
/// agent's next plan would be built on it as though it were.
#[tauri::command]
pub async fn remember(state: State<'_, AppState>, input: RememberInput) -> Answer<MemoryView> {
    record(&state.runtime, input).await
}

/// Rewrite a memory's content, and its confidence when one is given.
#[tauri::command]
pub async fn revise_memory(
    state: State<'_, AppState>,
    memory_id: String,
    content: String,
    confidence: Option<f32>,
) -> Answer<MemoryView> {
    let id: MemoryId = parse_id("memory", &memory_id)?;
    revise(&state.runtime, id, &content, confidence).await
}

/// Delete a memory. The audit record keeps what it said.
#[tauri::command]
pub async fn forget_memory(state: State<'_, AppState>, memory_id: String) -> Answer<()> {
    let id: MemoryId = parse_id("memory", &memory_id)?;
    state.runtime.forget_memory(id).await?;
    Ok(())
}

/// An agent's memories, shaped.
pub(crate) async fn memories(
    runtime: &Runtime,
    agent_id: AgentId,
    kind: Option<&str>,
) -> Answer<Vec<MemoryView>> {
    let kinds = match kind {
        None => Vec::new(),
        Some(text) => vec![known_kind(text)?],
    };
    let query = MemoryQuery {
        agent_id: Some(agent_id),
        kinds,
        limit: usize::MAX,
        ..MemoryQuery::default()
    };
    Ok(runtime
        .database()
        .memories()
        .query(&query)
        .await?
        .iter()
        .map(MemoryView::from)
        .collect())
}

/// Record a memory from the interface's input.
pub(crate) async fn record(runtime: &Runtime, input: RememberInput) -> Answer<MemoryView> {
    let agent_id: AgentId = parse_id("agent", &input.agent_id)?;
    let kind = known_kind(&input.kind)?;
    let memory = runtime
        .remember(
            agent_id,
            kind,
            &input.content,
            input.confidence.unwrap_or(FULL_CONFIDENCE),
        )
        .await?;
    Ok(MemoryView::from(&memory))
}

/// Revise a memory, keeping its confidence when none is given.
pub(crate) async fn revise(
    runtime: &Runtime,
    id: MemoryId,
    content: &str,
    confidence: Option<f32>,
) -> Answer<MemoryView> {
    let confidence = match confidence {
        Some(confidence) => confidence,
        None => runtime.database().memories().get(id).await?.confidence,
    };
    let memory = runtime.revise_memory(id, content, confidence).await?;
    Ok(MemoryView::from(&memory))
}

/// A memory kind, or a refusal naming the ones there are.
fn known_kind(text: &str) -> Answer<MemoryKind> {
    memory_kind(text).ok_or_else(|| {
        DesktopError::Rejected(format!(
            "`{text}` is not a kind of memory; expected one of {}",
            MemoryKind::ALL
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}
