//! Which processes sharing a data directory are driving runs.
//!
//! Several processes may open one installation at once: the desktop
//! application, an `agentos task run` waiting at a terminal, an
//! `agentos doctor`. Reaping abandoned runs is correct only for runs whose
//! process has gone, and the database cannot say which those are: a run in
//! `executing` looks the same whether its process is alive in another window or
//! died an hour ago.
//!
//! The operating system can say. Every process that drives a run holds a
//! shared lock on one file in the data directory for as long as it lives, and
//! the reaper reaps only while it can hold that lock exclusively, which is to
//! say while nobody, itself included, is driving anything. The kernel releases
//! a lock when its holder exits however it exits, so a crashed process stops
//! vouching for its runs at the moment it crashes, and a live one cannot have
//! its runs failed and its pending approvals expired underneath it.
//!
//! The lock is advisory and only AgentOS consults it. It defends against the
//! runtime misreading itself, not against anything else that can write the
//! database.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::OnceCell;

use crate::error::RuntimeError;

/// The lock file's name inside the data directory.
const LOCK_FILE: &str = "runs.lock";

/// How long to wait between attempts while a reaper holds the lock. Reaping
/// is a handful of statements, so this is rarely waited at all.
const RETRY: Duration = Duration::from_millis(25);

/// The shared lock a process holds while it drives runs, and the exclusive
/// one the reaper takes.
#[derive(Debug)]
pub(crate) struct RunsLock {
    path: PathBuf,
    /// The handle holding the shared lock, once this process has started a
    /// run. Kept for the life of the process rather than per run: releasing it
    /// between runs would let a reaper in.
    held: OnceCell<File>,
}

impl RunsLock {
    /// The lock for the data directory `data_dir`.
    pub(crate) fn in_directory(data_dir: &std::path::Path) -> Self {
        Self {
            path: data_dir.join(LOCK_FILE),
            held: OnceCell::new(),
        }
    }

    fn open(&self) -> Result<File, RuntimeError> {
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.path)
            .map_err(|source| RuntimeError::io(format!("opening {}", self.path.display()), source))
    }

    /// Declare that this process is driving runs, for as long as it lives.
    ///
    /// Waits while a reaper holds the lock, so a run never starts in the
    /// middle of a reap that could not see it.
    pub(crate) async fn hold(&self) -> Result<(), RuntimeError> {
        self.held
            .get_or_try_init(|| async {
                let file = self.open()?;
                loop {
                    match file.try_lock_shared() {
                        Ok(()) => return Ok(file),
                        Err(TryLockError::WouldBlock) => tokio::time::sleep(RETRY).await,
                        Err(TryLockError::Error(source)) => {
                            return Err(RuntimeError::io(
                                format!("locking {}", self.path.display()),
                                source,
                            ));
                        }
                    }
                }
            })
            .await
            .map(|_| ())
    }

    /// The exclusive lock, if no process is driving runs.
    ///
    /// `None` when any process holds the shared lock, this one included. The
    /// lock is released when the returned handle is dropped.
    pub(crate) fn try_exclusive(&self) -> Result<Option<File>, RuntimeError> {
        let file = self.open()?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(source)) => Err(RuntimeError::io(
                format!("locking {}", self.path.display()),
                source,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_process_driving_runs_keeps_the_reaper_out_until_it_is_gone() {
        let directory = tempfile::TempDir::new().unwrap();

        // Nobody is driving anything: the reaper may proceed.
        let reaper = RunsLock::in_directory(directory.path());
        assert!(reaper.try_exclusive().unwrap().is_some());

        // Another process starts a run. Two handles in one test stand in for
        // two processes: the lock is held per open file, not per process.
        let driver = RunsLock::in_directory(directory.path());
        driver.hold().await.unwrap();
        // Holding twice is holding once.
        driver.hold().await.unwrap();
        assert!(reaper.try_exclusive().unwrap().is_none());

        // The driver exits, however it exits.
        drop(driver);
        assert!(reaper.try_exclusive().unwrap().is_some());
    }

    #[tokio::test]
    async fn a_run_waits_out_a_reap_in_progress() {
        let directory = tempfile::TempDir::new().unwrap();
        let reaper = RunsLock::in_directory(directory.path());
        let reaping = reaper.try_exclusive().unwrap().unwrap();

        let driver = std::sync::Arc::new(RunsLock::in_directory(directory.path()));
        let starting = tokio::spawn({
            let driver = driver.clone();
            async move { driver.hold().await }
        });
        tokio::time::sleep(RETRY * 4).await;
        assert!(!starting.is_finished(), "a run started during a reap");

        drop(reaping);
        starting.await.unwrap().unwrap();
        assert!(reaper.try_exclusive().unwrap().is_none());
    }
}
