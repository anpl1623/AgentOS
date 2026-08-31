//! The seam between the tools and the operating system.
//!
//! Everything platform-specific is behind [`Desktop`], and everything above it —
//! how a call is scoped, when it is refused, what the approval card says — is
//! ordinary Rust that runs everywhere. That is deliberate: the interesting logic
//! is the authorisation, and authorisation that could only be tested on one
//! operating system would be tested on one operating system.
//!
//! Every method blocks. Callers put them on a blocking thread.

use std::fmt;

use tokio_util::sync::CancellationToken;

use crate::error::ComputerError;
use crate::input::{InputAction, Point};

/// The application currently receiving keyboard input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedApplication {
    /// What the application calls itself.
    pub name: String,
    /// The title of its focused window, which is content and may be hostile.
    pub title: String,
    /// The process it belongs to.
    pub pid: u32,
}

/// A window on the desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    /// The owning application's name.
    pub application: String,
    /// The window title.
    pub title: String,
    /// The process.
    pub pid: u32,
    /// Top-left corner, in points.
    pub origin: Point,
    /// Width in points.
    pub width: u32,
    /// Height in points.
    pub height: u32,
    /// Whether this window has keyboard focus.
    pub focused: bool,
    /// Whether it is minimised, and so not visible to a capture.
    pub minimised: bool,
}

/// A display.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayInfo {
    /// The operating system's name for it.
    pub name: String,
    /// Top-left corner in the desktop coordinate space, in points.
    pub origin: Point,
    /// Width in points.
    pub width: u32,
    /// Height in points.
    pub height: u32,
    /// Points-to-pixels ratio.
    pub scale: f32,
    /// Whether this is the primary display.
    pub primary: bool,
}

/// A captured image.
#[derive(Clone, PartialEq, Eq)]
pub struct Capture {
    /// PNG bytes.
    pub png: Vec<u8>,
    /// Width of the image in pixels.
    pub pixel_width: u32,
    /// Height of the image in pixels.
    pub pixel_height: u32,
    /// What was captured, for the audit trail and the taint source.
    pub target: String,
}

impl fmt::Debug for Capture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The bytes are an image of the operator's screen. Nothing good comes of
        // putting them in a log line.
        f.debug_struct("Capture")
            .field("png", &format_args!("{} bytes", self.png.len()))
            .field("pixel_width", &self.pixel_width)
            .field("pixel_height", &self.pixel_height)
            .field("target", &self.target)
            .finish()
    }
}

/// What the operating system has granted AgentOS.
///
/// Reported by `agentos doctor`, so an operator learns that input will fail
/// before an agent discovers it mid-task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    /// Whether this platform has a backend at all.
    pub supported: bool,
    /// Whether synthetic input is permitted, where the platform asks.
    pub input: Grant,
    /// Whether screen capture is permitted, where the platform asks.
    pub capture: Grant,
}

impl Preflight {
    /// A platform with no backend.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            supported: false,
            input: Grant::NotApplicable,
            capture: Grant::NotApplicable,
        }
    }
}

/// Whether one operating-system permission has been given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    /// Granted.
    Granted,
    /// Refused, or never asked for.
    Missing,
    /// This platform does not gate the capability.
    NotApplicable,
}

impl Grant {
    /// Whether this grant blocks the capability.
    #[must_use]
    pub const fn blocks(self) -> bool {
        matches!(self, Self::Missing)
    }
}

/// One physical desktop.
///
/// Implementations must not move the cursor, raise a window, or prompt for a
/// permission as a side effect of being asked a question: the read methods run
/// during planning, before anything has been authorised.
pub trait Desktop: Send + Sync + fmt::Debug {
    /// What the operating system has granted.
    fn preflight(&self) -> Preflight;

    /// The application receiving keyboard input.
    ///
    /// # Errors
    ///
    /// [`ComputerError::NoFocusedApplication`] when nothing is in front, and
    /// backend errors otherwise. Never guesses: an unknown target is a refusal,
    /// because a call that cannot name its target cannot be scoped.
    fn focused(&self) -> Result<FocusedApplication, ComputerError>;

    /// Every visible window.
    ///
    /// # Errors
    ///
    /// Backend failures.
    fn windows(&self) -> Result<Vec<WindowInfo>, ComputerError>;

    /// Every display.
    ///
    /// # Errors
    ///
    /// Backend failures.
    fn displays(&self) -> Result<Vec<DisplayInfo>, ComputerError>;

    /// Where the cursor is.
    ///
    /// # Errors
    ///
    /// Backend failures.
    fn cursor(&self) -> Result<Point, ComputerError>;

    /// Capture a whole display, or the primary one.
    ///
    /// # Errors
    ///
    /// Backend failures, including a missing screen-recording permission.
    fn capture_display(&self, name: Option<&str>) -> Result<Capture, ComputerError>;

    /// Capture only the focused window.
    ///
    /// # Errors
    ///
    /// Backend failures.
    fn capture_focused(&self) -> Result<Capture, ComputerError>;

    /// Perform an action, but only while `expected` is the application in front.
    ///
    /// The check is repeated before every individual event, so a focus change
    /// part-way through a long piece of typing stops the rest of it. What has
    /// already been delivered cannot be recalled — the error says how much got
    /// through, and the caller reports that rather than pretending the action
    /// did not happen. `cancel` is honoured at the same granularity.
    ///
    /// # Errors
    ///
    /// [`ComputerError::FocusChanged`] if the target moved,
    /// [`ComputerError::Cancelled`] if the run was stopped, and backend errors.
    fn perform(
        &self,
        action: &InputAction,
        expected: &str,
        cancel: &CancellationToken,
    ) -> Result<(), ComputerError>;
}

pub mod testing {
    //! A desktop that records instead of acting. **Tests only.**
    //!
    //! Shipped rather than hidden behind `cfg(test)` because the interesting
    //! thing to test about computer control is the policy written against it,
    //! and nobody should need a screen — or a CI runner with one — to test a
    //! policy. It is the counterpart of `agentos_tools::RecordingGate`.

    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{
        CancellationToken, Capture, ComputerError, Desktop, DisplayInfo, FocusedApplication, Grant,
        InputAction, Point, Preflight, WindowInfo,
    };

    /// A desktop with one window, which never sends anything anywhere.
    #[derive(Debug)]
    pub struct RecordingDesktop {
        front: Mutex<Option<FocusedApplication>>,
        performed: Mutex<Vec<(InputAction, String)>>,
        /// A focus change scheduled to happen after `n` reads, which is how a
        /// test reproduces the race between authorising a call and running it.
        pending: Mutex<Option<(usize, String)>>,
        reads: AtomicUsize,
    }

    impl RecordingDesktop {
        /// A desktop with `name` in front.
        #[must_use]
        pub fn in_front(name: &str) -> Self {
            Self {
                front: Mutex::new(Some(FocusedApplication {
                    name: name.to_owned(),
                    title: format!("{name} — a window"),
                    pid: 4321,
                })),
                performed: Mutex::new(Vec::new()),
                pending: Mutex::new(None),
                reads: AtomicUsize::new(0),
            }
        }

        /// A desktop where nothing has focus.
        #[must_use]
        pub fn with_nothing_in_front() -> Self {
            Self {
                front: Mutex::new(None),
                performed: Mutex::new(Vec::new()),
                pending: Mutex::new(None),
                reads: AtomicUsize::new(0),
            }
        }

        /// Pretend the window in front belongs to this very process, the way
        /// AgentOS's own window does.
        #[must_use]
        pub fn owned_by_this_process(self) -> Self {
            if let Some(application) = self
                .front
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_mut()
            {
                application.pid = std::process::id();
            }
            self
        }

        /// Move focus once the desktop has been read `reads` times.
        ///
        /// Authorisation reads it, then execution reads it again. Scheduling the
        /// change between the two is how a test reproduces a window moving while
        /// a human was looking at the approval card.
        pub fn switch_after(&self, reads: usize, name: &str) {
            *self
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some((reads, name.to_owned()));
        }

        /// Move focus, the way an operator or a hostile window can.
        pub fn switch_to(&self, name: &str) {
            *self
                .front
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(FocusedApplication {
                name: name.to_owned(),
                title: format!("{name} — a window"),
                pid: 4321,
            });
        }

        /// Everything that would have been sent.
        #[must_use]
        pub fn actions(&self) -> Vec<InputAction> {
            self.performed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .map(|(action, _)| action.clone())
                .collect()
        }
    }

    impl Desktop for RecordingDesktop {
        fn preflight(&self) -> Preflight {
            Preflight {
                supported: true,
                input: Grant::Granted,
                capture: Grant::Granted,
            }
        }

        fn focused(&self) -> Result<FocusedApplication, ComputerError> {
            let reads = self.reads.fetch_add(1, Ordering::SeqCst);
            let due = self
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .filter(|(after, _)| reads >= *after)
                .map(|(_, name)| name.clone());
            if let Some(name) = due {
                self.switch_to(&name);
            }
            self.front
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
                .ok_or(ComputerError::NoFocusedApplication)
        }

        fn windows(&self) -> Result<Vec<WindowInfo>, ComputerError> {
            let focused = self.focused()?;
            Ok(vec![WindowInfo {
                application: focused.name,
                title: focused.title,
                pid: focused.pid,
                origin: Point::new(0, 0),
                width: 800,
                height: 600,
                focused: true,
                minimised: false,
            }])
        }

        fn displays(&self) -> Result<Vec<DisplayInfo>, ComputerError> {
            Ok(vec![DisplayInfo {
                name: "Fake Display".to_owned(),
                origin: Point::new(0, 0),
                width: 1440,
                height: 900,
                scale: 2.0,
                primary: true,
            }])
        }

        fn cursor(&self) -> Result<Point, ComputerError> {
            Ok(Point::new(10, 10))
        }

        fn capture_display(&self, name: Option<&str>) -> Result<Capture, ComputerError> {
            Ok(Capture {
                png: b"not really a png".to_vec(),
                pixel_width: 2880,
                pixel_height: 1800,
                target: name.unwrap_or("Fake Display").to_owned(),
            })
        }

        fn capture_focused(&self) -> Result<Capture, ComputerError> {
            let focused = self.focused()?;
            Ok(Capture {
                png: b"not really a png".to_vec(),
                pixel_width: 1600,
                pixel_height: 1200,
                target: focused.name,
            })
        }

        fn perform(
            &self,
            action: &InputAction,
            expected: &str,
            cancel: &CancellationToken,
        ) -> Result<(), ComputerError> {
            if cancel.is_cancelled() {
                return Err(ComputerError::Cancelled {
                    delivered: 0,
                    total: action.event_count(),
                });
            }
            let actual = self.focused()?.name;
            if actual != expected {
                return Err(ComputerError::FocusChanged {
                    expected: expected.to_owned(),
                    actual,
                    delivered: 0,
                    total: action.event_count(),
                });
            }
            self.performed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((action.clone(), expected.to_owned()));
            Ok(())
        }
    }
}
