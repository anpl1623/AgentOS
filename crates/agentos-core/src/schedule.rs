//! Recurring work.
//!
//! A [`Schedule`] is a standing instruction to give an agent the same objective
//! again on a cadence. It is not a task: it *creates* tasks, one per firing, so
//! every occurrence keeps its own runs, traces, approvals and audit trail.
//!
//! # What a schedule cannot do
//!
//! A scheduled run happens with nobody watching. It is therefore run behind
//! [`DenyAllGate`](../../agentos_tools/struct.DenyAllGate.html): anything the
//! policy permits outright proceeds, and anything that would have asked a human
//! is refused with a note the model can read and re-plan around. A schedule
//! cannot approve on your behalf, and there is deliberately no setting that
//! makes it able to. An agent that needs a person to say yes needs a person.
//!
//! # Missed firings do not pile up
//!
//! The next occurrence is computed forward from the moment a schedule actually
//! fires, not from the slot it was supposed to fill. A machine that was asleep
//! for three days wakes up owing one run, not seventy-two.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::error::CoreError;
use crate::ids::{AgentId, ScheduleId, TaskId};

/// Which clock a cron expression is read against.
///
/// There are two, and no more: AgentOS carries no timezone database, so a named
/// IANA zone is not something it can honour. `Local` is the host's zone at the
/// moment the next occurrence is computed, which is what somebody writing
/// "every weekday at 09:00" almost always means, and which will shift by an hour
/// across a daylight-saving boundary. `Utc` is the one that does not move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Clock {
    /// Coordinated Universal Time.
    #[default]
    Utc,
    /// The host's local time.
    Local,
}

impl Clock {
    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utc => "utc",
            Self::Local => "local",
        }
    }
}

impl fmt::Display for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Clock {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "utc" => Ok(Self::Utc),
            "local" => Ok(Self::Local),
            other => Err(CoreError::UnknownVariant {
                kind: "clock",
                value: other.to_owned(),
            }),
        }
    }
}

/// How often a schedule fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cadence {
    /// Fire once, at the schedule's first occurrence, then finish.
    Once,
    /// Fire on a fixed interval.
    Every {
        /// Seconds between firings.
        seconds: u64,
    },
    /// Fire on a cron expression.
    Cron {
        /// The expression, in the form described by [`Cadence::validate`].
        expression: String,
        /// Which clock it is read against.
        clock: Clock,
    },
}

/// Shortest interval a schedule may use.
///
/// A minute. Anything faster is a polling loop wearing a schedule's clothes, and
/// each firing here is a whole agent run — a model request at minimum.
pub const MIN_INTERVAL_SECONDS: u64 = 60;

impl Cadence {
    /// Check that this cadence can actually be evaluated.
    ///
    /// Called before a schedule is stored, so a cron expression with a typo in
    /// it fails at the moment somebody writes it rather than silently never
    /// firing.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`] with the reason.
    pub fn validate(&self) -> Result<(), CoreError> {
        match self {
            Self::Once => Ok(()),
            Self::Every { seconds } => {
                if *seconds < MIN_INTERVAL_SECONDS {
                    return Err(CoreError::Invalid(format!(
                        "an interval of {seconds}s is shorter than the {MIN_INTERVAL_SECONDS}s \
                         minimum; each firing is a whole agent run"
                    )));
                }
                Ok(())
            }
            Self::Cron { expression, .. } => {
                normalise_cron(expression)
                    .parse::<cron::Schedule>()
                    .map_err(|error| {
                        CoreError::Invalid(format!(
                            "`{expression}` is not a cron expression: {error}"
                        ))
                    })?;
                Ok(())
            }
        }
    }

    /// The first occurrence strictly after `after`, if there is one.
    ///
    /// Returns `None` for [`Cadence::Once`], which has no occurrence beyond the
    /// one it was created with, and for a cron expression with nothing left in
    /// its future.
    #[must_use]
    pub fn next_after(&self, after: Timestamp) -> Option<Timestamp> {
        match self {
            Self::Once => None,
            Self::Every { seconds } => after
                .checked_add_signed(chrono::TimeDelta::try_seconds(*seconds as i64)?)
                .map(normalise),
            Self::Cron { expression, clock } => {
                let schedule = normalise_cron(expression).parse::<cron::Schedule>().ok()?;
                match clock {
                    Clock::Utc => schedule.after(&after).next().map(normalise),
                    Clock::Local => schedule
                        .after(&after.with_timezone(&chrono::Local))
                        .next()
                        .map(|next| normalise(next.with_timezone(&chrono::Utc))),
                }
            }
        }
    }

    /// Human-readable description, for `agentos schedule list` and traces.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Once => "once".to_owned(),
            Self::Every { seconds } => format!("every {seconds}s"),
            Self::Cron { expression, clock } => format!("cron `{expression}` ({clock})"),
        }
    }
}

/// Accept the five-field cron expression everybody writes.
///
/// The parser wants seconds first. A five-field expression is the near-universal
/// form — it is what `crontab` takes and what anybody reaching for this will
/// type — so it gains a leading `0` rather than an error message.
fn normalise_cron(expression: &str) -> String {
    let fields = expression.split_whitespace().count();
    if fields == 5 {
        format!("0 {}", expression.trim())
    } else {
        expression.trim().to_owned()
    }
}

/// Truncate to the precision every stored timestamp uses.
///
/// A computed occurrence must round-trip through the database unchanged, or the
/// scheduler will re-fire something it has already fired.
fn normalise(value: Timestamp) -> Timestamp {
    use chrono::Timelike;
    let nanos = value.nanosecond();
    if nanos >= 1_000_000_000 {
        return value;
    }
    value
        .with_nanosecond(nanos - (nanos % 1_000))
        .unwrap_or(value)
}

/// Whether a schedule is producing work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleStatus {
    /// Firing on its cadence.
    #[default]
    Active,
    /// Kept, but not firing.
    Paused,
    /// A `Once` schedule that has fired, or a cron with nothing left ahead.
    Finished,
}

impl ScheduleStatus {
    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Finished => "finished",
        }
    }

    /// Whether this schedule can still fire.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }
}

impl fmt::Display for ScheduleStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ScheduleStatus {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "paused" => Ok(Self::Paused),
            "finished" => Ok(Self::Finished),
            other => Err(CoreError::UnknownVariant {
                kind: "schedule status",
                value: other.to_owned(),
            }),
        }
    }
}

/// A standing instruction to give an agent the same objective on a cadence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    /// Identity.
    pub id: ScheduleId,
    /// The agent that will do the work.
    pub agent_id: AgentId,
    /// Unique, human-chosen name.
    pub name: String,
    /// The objective each firing gets. Trusted control-plane text, exactly like
    /// the objective on a task somebody typed.
    pub objective: String,
    /// How often it fires.
    pub cadence: Cadence,
    /// Whether it is firing.
    pub status: ScheduleStatus,
    /// When it next fires. `None` means never again.
    pub next_run_at: Option<Timestamp>,
    /// When it last fired.
    pub last_run_at: Option<Timestamp>,
    /// The task the last firing created.
    pub last_task_id: Option<TaskId>,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it was last modified.
    pub updated_at: Timestamp,
}

impl Schedule {
    /// Build an active schedule whose first occurrence is `first_run_at`.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`] if the cadence cannot be evaluated, or if the name
    /// or objective is empty.
    pub fn new(
        agent_id: AgentId,
        name: impl Into<String>,
        objective: impl Into<String>,
        cadence: Cadence,
        first_run_at: Timestamp,
    ) -> Result<Self, CoreError> {
        let name = name.into();
        let objective = objective.into();
        if name.trim().is_empty() {
            return Err(CoreError::Invalid("a schedule needs a name".to_owned()));
        }
        if objective.trim().is_empty() {
            return Err(CoreError::Invalid(
                "a schedule needs an objective".to_owned(),
            ));
        }
        cadence.validate()?;

        let now = crate::now();
        Ok(Self {
            id: ScheduleId::new(),
            agent_id,
            name,
            objective,
            cadence,
            status: ScheduleStatus::Active,
            next_run_at: Some(normalise(first_run_at)),
            last_run_at: None,
            last_task_id: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Whether this schedule is due at `now`.
    #[must_use]
    pub fn is_due(&self, now: Timestamp) -> bool {
        self.status.is_active() && self.next_run_at.is_some_and(|next| next <= now)
    }

    /// Record a firing and compute the next occurrence.
    ///
    /// `fired_at` is when the firing actually happened, not the slot it filled,
    /// which is what stops a machine that was asleep from waking up owing a
    /// backlog of runs nobody wants.
    pub fn record_firing(&mut self, fired_at: Timestamp, task_id: TaskId) {
        self.last_run_at = Some(fired_at);
        self.last_task_id = Some(task_id);
        self.next_run_at = self.cadence.next_after(fired_at);
        if self.next_run_at.is_none() {
            self.status = ScheduleStatus::Finished;
        }
        self.updated_at = crate::now();
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    fn at(text: &str) -> Timestamp {
        chrono::DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn a_five_field_expression_is_what_people_write() {
        let cadence = Cadence::Cron {
            expression: "0 9 * * *".to_owned(),
            clock: Clock::Utc,
        };
        cadence.validate().unwrap();
        assert_eq!(
            cadence.next_after(at("2026-09-01T08:30:00Z")),
            Some(at("2026-09-01T09:00:00Z"))
        );
    }

    #[test]
    fn a_six_field_expression_still_works() {
        let cadence = Cadence::Cron {
            expression: "30 0 9 * * *".to_owned(),
            clock: Clock::Utc,
        };
        assert_eq!(
            cadence.next_after(at("2026-09-01T08:30:00Z")),
            Some(at("2026-09-01T09:00:30Z"))
        );
    }

    #[test]
    fn weekdays_skip_the_weekend() {
        let cadence = Cadence::Cron {
            expression: "0 9 * * MON-FRI".to_owned(),
            clock: Clock::Utc,
        };
        // 2026-09-04 is a Friday; the next occurrence is the following Monday.
        assert_eq!(
            cadence.next_after(at("2026-09-04T09:00:00Z")),
            Some(at("2026-09-07T09:00:00Z"))
        );
    }

    #[test]
    fn a_typo_is_refused_when_it_is_written_not_when_it_fails_to_fire() {
        let cadence = Cadence::Cron {
            expression: "0 9 * * FUNDAY".to_owned(),
            clock: Clock::Utc,
        };
        let error = cadence.validate().unwrap_err();
        assert!(error.to_string().contains("not a cron expression"));
        assert_eq!(cadence.next_after(crate::now()), None);
    }

    #[test]
    fn intervals_below_a_minute_are_refused() {
        assert!(Cadence::Every { seconds: 30 }.validate().is_err());
        assert!(Cadence::Every { seconds: 60 }.validate().is_ok());
    }

    #[test]
    fn an_interval_counts_from_when_it_fired() {
        let cadence = Cadence::Every { seconds: 3600 };
        assert_eq!(
            cadence.next_after(at("2026-09-01T08:30:00Z")),
            Some(at("2026-09-01T09:30:00Z"))
        );
    }

    #[test]
    fn a_local_expression_tracks_the_host_clock() {
        let cadence = Cadence::Cron {
            expression: "0 9 * * *".to_owned(),
            clock: Clock::Local,
        };
        let next = cadence
            .next_after(at("2026-09-01T00:00:00Z"))
            .expect("a daily expression always has a next occurrence");
        // Whatever the host's offset is, the occurrence is 09:00 there.
        assert_eq!(
            next.with_timezone(&chrono::Local)
                .format("%H:%M")
                .to_string(),
            "09:00"
        );
    }

    #[test]
    fn a_sleeping_machine_wakes_owing_one_run_not_seventy_two() {
        let agent = AgentId::new();
        let mut schedule = Schedule::new(
            agent,
            "hourly",
            "Check the queue.",
            Cadence::Every { seconds: 3600 },
            at("2026-09-01T00:00:00Z"),
        )
        .unwrap();

        // Three days pass with the process down. It fires once, late.
        let woke = at("2026-09-04T00:00:00Z");
        assert!(schedule.is_due(woke));
        schedule.record_firing(woke, TaskId::new());

        assert_eq!(schedule.next_run_at, Some(at("2026-09-04T01:00:00Z")));
        assert!(!schedule.is_due(at("2026-09-04T00:30:00Z")));
    }

    #[test]
    fn a_one_shot_finishes_after_it_fires() {
        let mut schedule = Schedule::new(
            AgentId::new(),
            "once",
            "Do the thing.",
            Cadence::Once,
            at("2026-09-01T00:00:00Z"),
        )
        .unwrap();

        assert!(schedule.is_due(at("2026-09-01T00:00:01Z")));
        schedule.record_firing(at("2026-09-01T00:00:01Z"), TaskId::new());

        assert_eq!(schedule.status, ScheduleStatus::Finished);
        assert_eq!(schedule.next_run_at, None);
        assert!(!schedule.is_due(at("2027-01-01T00:00:00Z")));
    }

    #[test]
    fn a_paused_schedule_is_never_due() {
        let mut schedule = Schedule::new(
            AgentId::new(),
            "paused",
            "Do the thing.",
            Cadence::Every { seconds: 60 },
            at("2026-09-01T00:00:00Z"),
        )
        .unwrap();
        schedule.status = ScheduleStatus::Paused;
        assert!(!schedule.is_due(at("2030-01-01T00:00:00Z")));
    }

    #[test]
    fn schedules_need_a_name_and_an_objective() {
        let agent = AgentId::new();
        let now = chrono::Utc.timestamp_opt(0, 0).unwrap();
        assert!(Schedule::new(agent, "  ", "objective", Cadence::Once, now).is_err());
        assert!(Schedule::new(agent, "name", "  ", Cadence::Once, now).is_err());
    }

    #[test]
    fn computed_occurrences_survive_the_database_round_trip() {
        // The scheduler compares `next_run_at` against a stored value. A
        // sub-microsecond difference would make it re-fire what it just fired.
        let cadence = Cadence::Every { seconds: 90 };
        let next = cadence.next_after(crate::now()).unwrap();
        let text = crate::format_timestamp(&next);
        let parsed = chrono::DateTime::parse_from_rfc3339(&text)
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(parsed, next);
    }
}
