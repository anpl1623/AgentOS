//! Browser sessions, one per run.
//!
//! A run gets its own browser process and its own profile directory, so two
//! agents working at once cannot see each other's cookies, storage or history.
//! Sessions are launched lazily — an agent that never browses never starts a
//! browser — and closed when the run ends.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agentos_core::ids::TaskRunId;
use chromiumoxide::Page;
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures::StreamExt;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::error::BrowserError;
use crate::locate;

/// How long to wait for the browser process to become usable.
pub const LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a single CDP command may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Viewport used unless configured otherwise.
pub const VIEWPORT: (u32, u32) = (1280, 900);

/// How long each stage of shutdown may take before escalating.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// How a browser should be launched.
#[derive(Debug, Clone)]
pub struct BrowserOptions {
    /// Explicit executable, overriding discovery.
    pub executable: Option<PathBuf>,
    /// Show a window. Off by default; on when a human wants to watch.
    pub headed: bool,
    /// Directory for per-run browser profiles.
    pub profile_root: PathBuf,
}

impl BrowserOptions {
    /// Options rooted at a profile directory.
    #[must_use]
    pub fn new(profile_root: impl Into<PathBuf>) -> Self {
        Self {
            executable: None,
            headed: false,
            profile_root: profile_root.into(),
        }
    }

    /// Use a specific executable.
    #[must_use]
    pub fn with_executable(mut self, executable: Option<PathBuf>) -> Self {
        self.executable = executable;
        self
    }

    /// Show the browser window.
    #[must_use]
    pub const fn headed(mut self, headed: bool) -> Self {
        self.headed = headed;
        self
    }
}

/// One run's browser.
#[derive(Debug)]
pub struct BrowserSession {
    browser: Mutex<Browser>,
    page: Mutex<Page>,
    /// Drives the CDP event loop. Aborted on close.
    handler: Mutex<Option<JoinHandle<()>>>,
    profile: PathBuf,
}

impl BrowserSession {
    /// Launch a browser and open a blank page.
    ///
    /// # Errors
    ///
    /// [`BrowserError::NotFound`] if no browser is installed, or
    /// [`BrowserError::Launch`] if it will not start.
    pub async fn launch(options: &BrowserOptions, run_id: TaskRunId) -> Result<Self, BrowserError> {
        let executable = locate::locate(options.executable.as_deref())
            .ok_or_else(|| BrowserError::NotFound(locate::install_hint()))?;

        let profile = options.profile_root.join(run_id.to_string());
        std::fs::create_dir_all(&profile).map_err(|source| BrowserError::Launch {
            message: format!(
                "creating the profile directory {}: {source}",
                profile.display()
            ),
        })?;

        let mut builder = BrowserConfig::builder()
            .chrome_executable(&executable)
            .user_data_dir(&profile)
            .window_size(VIEWPORT.0, VIEWPORT.1)
            .launch_timeout(LAUNCH_TIMEOUT)
            .request_timeout(REQUEST_TIMEOUT)
            // No first-run dialogs, no default browser prompt, no crash-restore
            // bubble: an agent cannot dismiss any of them.
            // `Arg` adds the leading dashes itself, so these are written bare;
            // passing "--foo" produces "----foo", which Chrome ignores silently.
            .arg("no-first-run")
            .arg("no-default-browser-check")
            .arg("disable-background-networking")
            .arg("disable-sync")
            .arg("disable-extensions")
            .arg("hide-crash-restore-bubble")
            .arg("mute-audio");

        // `HeadlessMode` is not re-exported, so the mode is selected through the
        // builder's own helpers rather than by naming the enum.
        builder = if options.headed {
            builder.with_head()
        } else {
            builder.new_headless_mode()
        };

        let config = builder
            .build()
            .map_err(|message| BrowserError::Launch { message })?;

        let (browser, mut handler) =
            Browser::launch(config)
                .await
                .map_err(|source| BrowserError::Launch {
                    message: format!("{source} (using {})", executable.display()),
                })?;

        // The handler future must be polled continuously or every command
        // deadlocks waiting for a response nobody is reading.
        let handler_task = tokio::spawn(async move { while handler.next().await.is_some() {} });

        let page =
            browser
                .new_page("about:blank")
                .await
                .map_err(|source| BrowserError::Launch {
                    message: source.to_string(),
                })?;

        Ok(Self {
            browser: Mutex::new(browser),
            page: Mutex::new(page),
            handler: Mutex::new(Some(handler_task)),
            profile,
        })
    }

    /// The page this session drives.
    pub async fn page(&self) -> Page {
        self.page.lock().await.clone()
    }

    /// The current URL, if the page has one.
    pub async fn current_url(&self) -> Option<String> {
        self.page()
            .await
            .url()
            .await
            .ok()
            .flatten()
            .filter(|url| url != "about:blank")
    }

    /// Close the browser and remove its profile.
    ///
    /// Order matters. `Browser::close` is a CDP command, so it needs a reply —
    /// and replies only arrive while the handler task is being polled. Aborting
    /// the handler first makes close wait forever for an answer nobody is
    /// listening for.
    ///
    /// Best-effort throughout: a browser that has already died is not an error,
    /// and the caller is often cleaning up after something that already went
    /// wrong. Every step is time-boxed, and anything still alive at the end is
    /// killed, so shutdown cannot hang a run or leak a process.
    pub async fn close(&self) {
        let mut browser = self.browser.lock().await;

        // 1. Ask nicely, while the handler is still driving the connection.
        let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, browser.close()).await;

        // 2. Wait for the process, then stop driving the connection.
        let exited = tokio::time::timeout(SHUTDOWN_TIMEOUT, browser.wait())
            .await
            .is_ok();
        if let Some(task) = self.handler.lock().await.take() {
            task.abort();
        }

        // 3. Anything still running gets killed. `kill_on_drop` is not set on
        //    this child, so this is the only thing that reaps a stuck browser.
        if !exited {
            tracing::warn!("browser did not exit cleanly; killing it");
            let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, browser.kill()).await;
        }

        drop(browser);
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

/// Browser sessions, keyed by run.
///
/// Tools are shared across every run, so per-run state cannot live in the tool.
/// It lives here, and the runtime releases it when a run finishes.
#[derive(Debug)]
pub struct BrowserPool {
    options: BrowserOptions,
    sessions: Mutex<HashMap<TaskRunId, Arc<BrowserSession>>>,
}

impl BrowserPool {
    /// An empty pool.
    #[must_use]
    pub fn new(options: BrowserOptions) -> Self {
        Self {
            options,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Get the session for a run, launching one if needed.
    ///
    /// # Errors
    ///
    /// [`BrowserError`] if the browser cannot be found or started.
    pub async fn session(&self, run_id: TaskRunId) -> Result<Arc<BrowserSession>, BrowserError> {
        // The lock is held across the launch so two concurrent tool calls in the
        // same run cannot each start a browser.
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(&run_id) {
            return Ok(session.clone());
        }

        let session = Arc::new(BrowserSession::launch(&self.options, run_id).await?);
        sessions.insert(run_id, session.clone());
        Ok(session)
    }

    /// Whether a run has a live session.
    pub async fn has_session(&self, run_id: TaskRunId) -> bool {
        self.sessions.lock().await.contains_key(&run_id)
    }

    /// The session for a run, if one is already open.
    ///
    /// Never launches. Planning runs before authorisation, so it must not be
    /// able to start a browser process as a side effect of being asked what a
    /// call would do.
    pub async fn existing_session(&self, run_id: TaskRunId) -> Option<Arc<BrowserSession>> {
        self.sessions.lock().await.get(&run_id).cloned()
    }

    /// Close and forget a run's session.
    pub async fn close_run(&self, run_id: TaskRunId) {
        let session = self.sessions.lock().await.remove(&run_id);
        if let Some(session) = session {
            session.close().await;
        }
    }

    /// Close every session.
    pub async fn close_all(&self) {
        let sessions: Vec<Arc<BrowserSession>> =
            self.sessions.lock().await.drain().map(|(_, s)| s).collect();
        for session in sessions {
            session.close().await;
        }
    }

    /// How many sessions are open.
    pub async fn len(&self) -> usize {
        self.sessions.lock().await.len()
    }

    /// Whether the pool is empty.
    pub async fn is_empty(&self) -> bool {
        self.sessions.lock().await.is_empty()
    }
}
