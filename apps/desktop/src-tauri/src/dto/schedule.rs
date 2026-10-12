//! Schedules, their cadences, and the input that creates one.

use agentos_core::schedule::{Cadence, Schedule};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{at, maybe_at};

/// A cadence as a form edits it.
///
/// Flat rather than tagged, so one form can bind every field and switch
/// between kinds without rebuilding its state. The fields that do not belong
/// to `kind` must be empty; a combination that names two cadences is refused,
/// not resolved by guessing which one was meant.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct CadenceInput {
    /// `once`, `every` or `cron`.
    pub kind: String,
    /// Seconds between firings, for `every`.
    #[ts(type = "number | null")]
    pub seconds: Option<u64>,
    /// The expression, for `cron`. Five fields or six.
    pub expression: Option<String>,
    /// `utc` or `local`, for `cron`. UTC when absent.
    pub clock: Option<String>,
}

/// A cadence, for display.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct CadenceView {
    /// `once`, `every` or `cron`.
    pub kind: String,
    /// Seconds between firings, for `every`.
    #[ts(type = "number | null")]
    pub seconds: Option<u64>,
    /// The expression, for `cron`.
    pub expression: Option<String>,
    /// `utc` or `local`, for `cron`.
    pub clock: Option<String>,
    /// The runtime's own description, as the CLI prints it.
    pub description: String,
}

impl From<&Cadence> for CadenceView {
    fn from(cadence: &Cadence) -> Self {
        let (kind, seconds, expression, clock) = match cadence {
            Cadence::Once => ("once", None, None, None),
            Cadence::Every { seconds } => ("every", Some(*seconds), None, None),
            Cadence::Cron { expression, clock } => (
                "cron",
                None,
                Some(expression.clone()),
                Some(clock.as_str().to_owned()),
            ),
        };
        Self {
            kind: kind.to_owned(),
            seconds,
            expression,
            clock,
            description: cadence.describe(),
        }
    }
}

/// What checking a cadence found, before anything is stored.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct CadencePreview {
    /// Whether it can be evaluated.
    pub valid: bool,
    /// Why not, when it cannot.
    pub error: Option<String>,
    /// The runtime's description of it, when it can.
    pub description: Option<String>,
    /// The next few occurrences after now, soonest first.
    ///
    /// Empty for `once`, whose single moment is the schedule's first run and
    /// not a property of the cadence.
    pub next_runs: Vec<String>,
}

/// A schedule, with its agent's name.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ScheduleView {
    /// Identity.
    pub id: String,
    /// Unique, human-chosen name.
    pub name: String,
    /// The agent that does the work.
    pub agent_id: String,
    /// Its name, or `(deleted)`.
    pub agent_name: String,
    /// The objective each firing is given.
    pub objective: String,
    /// How often it fires.
    pub cadence: CadenceView,
    /// `active`, `paused` or `finished`.
    pub status: String,
    /// When it next fires; absent when it never will again.
    pub next_run_at: Option<String>,
    /// When it last fired.
    pub last_run_at: Option<String>,
    /// The task the last firing created.
    pub last_task_id: Option<String>,
    /// When it was created.
    pub created_at: String,
}

impl ScheduleView {
    /// Build a view, given the agent's name.
    #[must_use]
    pub fn new(schedule: &Schedule, agent_name: &str) -> Self {
        Self {
            id: schedule.id.to_string(),
            name: schedule.name.clone(),
            agent_id: schedule.agent_id.to_string(),
            agent_name: agent_name.to_owned(),
            objective: schedule.objective.clone(),
            cadence: CadenceView::from(&schedule.cadence),
            status: schedule.status.as_str().to_owned(),
            next_run_at: maybe_at(schedule.next_run_at.as_ref()),
            last_run_at: maybe_at(schedule.last_run_at.as_ref()),
            last_task_id: schedule.last_task_id.map(|id| id.to_string()),
            created_at: at(&schedule.created_at),
        }
    }
}

/// What the interface sends to create a schedule.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct CreateScheduleInput {
    /// The agent that will do the work.
    pub agent_id: String,
    /// Unique name.
    pub name: String,
    /// The objective each firing gets.
    pub objective: String,
    /// How often it fires.
    pub cadence: CadenceInput,
    /// The first occurrence, as RFC 3339. Required for `once`; otherwise the
    /// cadence's next occurrence after now.
    pub first_run_at: Option<String>,
}
