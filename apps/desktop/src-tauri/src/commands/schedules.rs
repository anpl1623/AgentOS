//! Standing instructions: schedules and their cadences.
//!
//! Each change goes through the runtime's operator layer, which records it, so
//! a schedule created here and one created with `agentos schedule create` leave
//! the same trail.

use agentos_core::ids::ScheduleId;
use agentos_core::schedule::{Cadence, Clock, Schedule};
use agentos_runtime::Runtime;
use tauri::State;

use super::{Answer, DesktopError, agent_names, parse_id};
use crate::dto::{CadenceInput, CadencePreview, CreateScheduleInput, ScheduleView};
use crate::state::AppState;

/// How many occurrences a cadence preview lists.
const PREVIEWED_RUNS: usize = 5;

/// The refusal for a cadence that names no kind, or more than one.
const ONE_CADENCE: &str = "choose exactly one of once, every or cron";

impl TryFrom<CadenceInput> for Cadence {
    type Error = DesktopError;

    /// Read a form's cadence.
    ///
    /// Only the shape is checked here. Whether an interval is long enough or
    /// an expression parses is [`Cadence::validate`]'s question, asked by
    /// whoever stores or previews the result, so the desktop cannot drift from
    /// the minimums the CLI enforces.
    fn try_from(input: CadenceInput) -> Result<Self, Self::Error> {
        let rejected = || DesktopError::Rejected(ONE_CADENCE.to_owned());
        match (
            input.kind.as_str(),
            input.seconds,
            input.expression,
            input.clock,
        ) {
            ("once", None, None, None) => Ok(Self::Once),
            ("every", Some(seconds), None, None) => Ok(Self::Every { seconds }),
            ("cron", None, Some(expression), clock) => {
                let clock = match clock.as_deref() {
                    None => Clock::Utc,
                    Some(text) => text.parse::<Clock>().map_err(|_| {
                        DesktopError::Rejected(format!(
                            "`{text}` is not a clock; a cron expression is read in utc or local"
                        ))
                    })?,
                };
                Ok(Self::Cron { expression, clock })
            }
            _ => Err(rejected()),
        }
    }
}

/// Every schedule, newest first, with its agent's name.
#[tauri::command]
pub async fn list_schedules(state: State<'_, AppState>) -> Answer<Vec<ScheduleView>> {
    schedule_views(&state.runtime).await
}

/// Create a schedule.
#[tauri::command]
pub async fn create_schedule(
    state: State<'_, AppState>,
    input: CreateScheduleInput,
) -> Answer<ScheduleView> {
    create(&state.runtime, input).await
}

/// Stop a schedule firing, or start it again.
///
/// Resuming computes the next occurrence forward from now, so a schedule
/// paused over a weekend does not wake owing every firing it missed.
#[tauri::command]
pub async fn set_schedule_paused(
    state: State<'_, AppState>,
    schedule_id: String,
    paused: bool,
) -> Answer<ScheduleView> {
    let id: ScheduleId = parse_id("schedule", &schedule_id)?;
    let runtime = &state.runtime;
    let schedule = runtime.set_schedule_paused(id, paused).await?;
    view_of(runtime, &schedule).await
}

/// Delete a schedule. The tasks it already created are kept, with their
/// traces and audit records.
#[tauri::command]
pub async fn delete_schedule(state: State<'_, AppState>, schedule_id: String) -> Answer<()> {
    let id: ScheduleId = parse_id("schedule", &schedule_id)?;
    state.runtime.delete_schedule(id).await?;
    Ok(())
}

/// Check a cadence without storing anything.
///
/// The counterpart of `check_policy`: a cron expression with a typo is refused
/// while it is being typed, rather than stored as a schedule that silently
/// never fires.
#[tauri::command]
pub async fn check_cadence(cadence: CadenceInput) -> Answer<CadencePreview> {
    preview(cadence)
}

/// Every schedule, shaped.
pub(crate) async fn schedule_views(runtime: &Runtime) -> Answer<Vec<ScheduleView>> {
    let names = agent_names(runtime).await?;
    Ok(runtime
        .schedules()
        .await?
        .iter()
        .map(|schedule| {
            let name = names
                .get(&schedule.agent_id)
                .map_or("(deleted)", String::as_str);
            ScheduleView::new(schedule, name)
        })
        .collect())
}

/// One schedule, shaped.
async fn view_of(runtime: &Runtime, schedule: &Schedule) -> Answer<ScheduleView> {
    let name = runtime
        .database()
        .agents()
        .find(schedule.agent_id)
        .await?
        .map_or_else(|| "(deleted)".to_owned(), |agent| agent.name);
    Ok(ScheduleView::new(schedule, &name))
}

/// Create a schedule from the interface's input.
pub(crate) async fn create(runtime: &Runtime, input: CreateScheduleInput) -> Answer<ScheduleView> {
    let agent_id = parse_id("agent", &input.agent_id)?;
    let cadence = Cadence::try_from(input.cadence)?;
    let first_run_at = first_run_at(&cadence, input.first_run_at.as_deref())?;
    let schedule = runtime
        .create_schedule(
            agent_id,
            input.name.trim(),
            &input.objective,
            cadence,
            first_run_at,
        )
        .await?;
    view_of(runtime, &schedule).await
}

/// When a new schedule first fires.
///
/// A time the operator gave wins. Otherwise a recurring cadence computes its
/// own first occurrence, and a one-shot is refused: with no time it would fire
/// on the next tick, which is never what somebody creating a schedule meant.
pub(crate) fn first_run_at(
    cadence: &Cadence,
    given: Option<&str>,
) -> Answer<agentos_core::Timestamp> {
    match given.map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => parse_time(text),
        None if matches!(cadence, Cadence::Once) => Err(DesktopError::Rejected(
            "a schedule that fires once needs a time to say when".to_owned(),
        )),
        None => Ok(cadence
            .next_after(agentos_core::now())
            .unwrap_or_else(agentos_core::now)),
    }
}

/// Read an RFC 3339 moment from the interface.
pub(crate) fn parse_time(text: &str) -> Answer<agentos_core::Timestamp> {
    text.parse::<agentos_core::Timestamp>().map_err(|_| {
        DesktopError::Rejected(format!(
            "`{text}` is not a moment the runtime can read; expected RFC 3339, such as \
             2026-10-06T09:00:00Z"
        ))
    })
}

/// Check a cadence and list where it would fire next.
pub(crate) fn preview(input: CadenceInput) -> Answer<CadencePreview> {
    let cadence = Cadence::try_from(input)?;
    if let Err(error) = cadence.validate() {
        return Ok(CadencePreview {
            valid: false,
            error: Some(error.to_string()),
            description: None,
            next_runs: Vec::new(),
        });
    }

    let mut next_runs = Vec::with_capacity(PREVIEWED_RUNS);
    let mut after = agentos_core::now();
    while next_runs.len() < PREVIEWED_RUNS {
        let Some(next) = cadence.next_after(after) else {
            break;
        };
        next_runs.push(agentos_core::format_timestamp(&next));
        after = next;
    }

    Ok(CadencePreview {
        valid: true,
        error: None,
        description: Some(cadence.describe()),
        next_runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(kind: &str, seconds: Option<u64>, expression: Option<&str>) -> CadenceInput {
        CadenceInput {
            kind: kind.to_owned(),
            seconds,
            expression: expression.map(str::to_owned),
            clock: None,
        }
    }

    #[test]
    fn a_cadence_naming_two_kinds_or_none_is_refused() {
        for mismatched in [
            input("once", Some(60), None),
            input("every", None, None),
            input("every", Some(60), Some("* * * * *")),
            input("cron", Some(60), Some("* * * * *")),
            input("cron", None, None),
            input("hourly", None, None),
            CadenceInput {
                clock: Some("utc".to_owned()),
                ..input("every", Some(60), None)
            },
        ] {
            let error = Cadence::try_from(mismatched.clone()).unwrap_err();
            assert_eq!(error.to_string(), ONE_CADENCE, "{mismatched:?}");
        }

        assert_eq!(
            Cadence::try_from(input("every", Some(30), None)).unwrap(),
            Cadence::Every { seconds: 30 },
            "the minimum is the runtime's to enforce, not the shape check's"
        );
        assert!(
            Cadence::try_from(CadenceInput {
                clock: Some("pacific".to_owned()),
                ..input("cron", None, Some("0 9 * * *"))
            })
            .is_err()
        );
    }

    #[test]
    fn a_preview_lists_five_runs_or_says_why_there_are_none() {
        let hourly = preview(input("cron", None, Some("0 * * * *"))).unwrap();
        assert!(hourly.valid);
        assert_eq!(hourly.next_runs.len(), PREVIEWED_RUNS);
        assert!(hourly.next_runs.windows(2).all(|pair| pair[0] < pair[1]));

        let typo = preview(input("cron", None, Some("0 25 * * *"))).unwrap();
        assert!(!typo.valid);
        assert!(typo.error.is_some());

        let fast = preview(input("every", Some(10), None)).unwrap();
        assert!(!fast.valid, "a ten-second interval is under the minimum");

        let once = preview(input("once", None, None)).unwrap();
        assert!(once.valid);
        assert!(once.next_runs.is_empty());
    }

    #[test]
    fn a_one_shot_needs_a_time_and_a_recurring_cadence_finds_its_own() {
        let error = first_run_at(&Cadence::Once, None).unwrap_err();
        assert!(error.to_string().contains("needs a time"));
        assert!(first_run_at(&Cadence::Once, Some("  ")).is_err());

        let given = first_run_at(&Cadence::Once, Some("2030-01-01T09:00:00Z")).unwrap();
        assert_eq!(
            agentos_core::format_timestamp(&given),
            "2030-01-01T09:00:00.000000Z"
        );

        let every = first_run_at(&Cadence::Every { seconds: 3600 }, None).unwrap();
        assert!(every > agentos_core::now());

        assert!(first_run_at(&Cadence::Once, Some("next tuesday")).is_err());
    }
}
