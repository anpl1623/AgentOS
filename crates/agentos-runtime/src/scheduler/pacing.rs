//! How a scheduler is paced, and whether the desktop application runs one.
//!
//! The options a running scheduler is given, the bounds an operator's choice
//! of them is held to, and the preference saved so that a scheduler somebody
//! turned on is running again the next time the application opens.

use std::time::Duration;

use agentos_persistence::settings::keys;
use serde::{Deserialize, Serialize};

use crate::{Runtime, RuntimeError};

/// How the scheduler paces itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerOptions {
    /// How long to wait between ticks.
    pub tick: Duration,
    /// How many runs may be in flight at once.
    pub max_concurrent_runs: usize,
    /// How many due schedules, and how many runnable tasks, one tick considers.
    pub batch: i64,
}

/// Default gap between ticks.
///
/// Thirty seconds. The finest cadence a schedule can express is a minute, so
/// anything faster is a busy loop, and anything much slower would make a
/// once-a-minute schedule visibly late.
pub const DEFAULT_TICK: Duration = Duration::from_secs(30);

impl Default for SchedulerOptions {
    fn default() -> Self {
        Self {
            tick: DEFAULT_TICK,
            // One. Unattended runs cost money and touch the world, and an
            // operator who wants more of that at once should have to say so.
            max_concurrent_runs: 1,
            batch: 32,
        }
    }
}

impl SchedulerOptions {
    /// Set the gap between ticks.
    #[must_use]
    pub const fn with_tick(mut self, tick: Duration) -> Self {
        self.tick = tick;
        self
    }

    /// Set how many runs may be in flight at once.
    #[must_use]
    pub const fn with_max_concurrent_runs(mut self, max: usize) -> Self {
        self.max_concurrent_runs = max;
        self
    }
}

/// The shortest gap between ticks an operator may ask for.
///
/// Five seconds. A tick is a handful of queries, so this is not about cost; it
/// is the point below which a scheduler is a busy loop that happens to sleep.
pub const MIN_TICK_SECONDS: u64 = 5;

/// Whether the desktop application should run a scheduler, and how.
///
/// Persisted through the settings repository, so that turning the scheduler on
/// is a decision that outlives the window it was made in. The default is off:
/// unattended work starts only once somebody has asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerPreference {
    /// Whether a scheduler should be running.
    pub enabled: bool,
    /// Seconds between ticks.
    pub tick_seconds: u64,
    /// How many runs it may have in flight at once.
    pub max_concurrent_runs: u32,
}

impl Default for SchedulerPreference {
    fn default() -> Self {
        let options = SchedulerOptions::default();
        Self {
            enabled: false,
            tick_seconds: options.tick.as_secs(),
            max_concurrent_runs: u32::try_from(options.max_concurrent_runs).unwrap_or(1),
        }
    }
}

impl SchedulerPreference {
    /// The options a scheduler started from this preference runs with.
    #[must_use]
    pub fn options(&self) -> SchedulerOptions {
        SchedulerOptions::default()
            .with_tick(Duration::from_secs(self.tick_seconds))
            .with_max_concurrent_runs(usize::try_from(self.max_concurrent_runs).unwrap_or(1))
    }

    /// Refuse pacing a scheduler should not run with.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for a tick under [`MIN_TICK_SECONDS`] or a
    /// concurrency of zero, which would be a scheduler that starts nothing.
    pub fn validate(&self) -> Result<(), RuntimeError> {
        if self.tick_seconds < MIN_TICK_SECONDS {
            return Err(RuntimeError::Rejected(format!(
                "a scheduler ticks at most every {MIN_TICK_SECONDS} seconds"
            )));
        }
        if self.max_concurrent_runs == 0 {
            return Err(RuntimeError::Rejected(
                "a scheduler that may run nothing at once would start nothing".to_owned(),
            ));
        }
        Ok(())
    }
}

/// A scheduler starting or stopping, for [`Runtime::record_scheduler_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerTransition {
    /// It took the lease and is about to tick.
    Started {
        /// Started because a saved [`SchedulerPreference`] said so as the
        /// application opened, rather than because somebody asked just now.
        on_launch: bool,
    },
    /// It stopped ticking and its runs have drained.
    Stopped,
}

impl Runtime {
    /// The saved scheduler preference, or the default when none is saved.
    ///
    /// A stored value that does not read is treated as no preference, which is
    /// off: a damaged setting must not be what starts unattended work.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Database`] if the setting cannot be read.
    pub async fn scheduler_preference(&self) -> Result<SchedulerPreference, RuntimeError> {
        let Some(stored) = self.database.settings().get(keys::SCHEDULER).await? else {
            return Ok(SchedulerPreference::default());
        };
        Ok(serde_json::from_str(&stored).unwrap_or_else(|error| {
            tracing::warn!(%error, "the saved scheduler preference does not read; treating it as off");
            SchedulerPreference::default()
        }))
    }

    /// Save the scheduler preference.
    ///
    /// Saving it starts and stops nothing; the client that owns the scheduler
    /// does that, and records it with [`Runtime::record_scheduler_state`].
    ///
    /// # Errors
    ///
    /// [`RuntimeError::Rejected`] for pacing [`SchedulerPreference::validate`]
    /// refuses, and [`RuntimeError::Database`] if the setting cannot be
    /// written.
    pub async fn set_scheduler_preference(
        &self,
        enabled: bool,
        tick_seconds: u64,
        max_concurrent_runs: u32,
    ) -> Result<SchedulerPreference, RuntimeError> {
        let preference = SchedulerPreference {
            enabled,
            tick_seconds,
            max_concurrent_runs,
        };
        preference.validate()?;
        let stored = serde_json::to_string(&preference)
            .map_err(|error| RuntimeError::Rejected(error.to_string()))?;
        self.database
            .settings()
            .set(keys::SCHEDULER, &stored)
            .await?;
        Ok(preference)
    }
}
