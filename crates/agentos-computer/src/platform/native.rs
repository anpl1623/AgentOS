//! The macOS and Windows backend.
//!
//! Input goes through `enigo`, screen reads through `xcap`. Neither needs any
//! unsafe code here, which matters: this crate is the one place in AgentOS that
//! reaches the raw machine, and `unsafe_code = "forbid"` still holds.
//!
//! Three things in this file are load-bearing rather than incidental.
//!
//! **Focus is re-read before every event.** Not once per call — once per
//! keystroke. A twenty-character password typed into a window that changes
//! half-way through is two windows' worth of secret, and checking once at the
//! start would not notice.
//!
//! **Input is serialised process-wide.** Concurrent runs share one cursor and
//! one keyboard. Nothing in the runtime serialises tool calls across runs, so if
//! two runs type at once the result is interleaved characters in whichever
//! window is in front. The lock makes an action atomic with respect to other
//! actions; it cannot make it atomic with respect to the operator.
//!
//! **Nothing is held between calls.** The connection to the window server is
//! opened per action and dropped after it. Building the registry must not touch
//! the screen — `agentos tools` lists the catalogue and that is not a reason to
//! ask macOS for the Accessibility permission.

use std::sync::{Mutex, MutexGuard};

use enigo::{Direction, Enigo, Keyboard, Mouse, Settings};
use tokio_util::sync::CancellationToken;

use crate::desktop::{
    Capture, Desktop, DisplayInfo, FocusedApplication, Grant, Preflight, WindowInfo,
};
use crate::error::ComputerError;
use crate::input::{Axis, Button, InputAction, Key, Modifier, Point};

/// Serialises input across every run in this process.
static INPUT: Mutex<()> = Mutex::new(());

/// The real desktop.
#[derive(Debug)]
pub(crate) struct NativeDesktop;

impl NativeDesktop {
    pub(crate) const fn new() -> Self {
        Self
    }

    /// Open a connection to the window server for one action.
    fn enigo() -> Result<Enigo, ComputerError> {
        Enigo::new(&Settings {
            // Prompting mid-run puts a system dialogue in front of the operator
            // with no explanation of what asked for it. `agentos doctor` reports
            // the missing grant instead, and says where to give it.
            open_prompt_to_get_permissions: false,
            ..Settings::default()
        })
        .map_err(|error| match error {
            enigo::NewConError::NoPermission => ComputerError::NotPermitted {
                permission: "Accessibility",
                remedy: INPUT_REMEDY,
            },
            other => {
                ComputerError::backend("connecting to the window server", format!("{other:?}"))
            }
        })
    }

    fn lock() -> MutexGuard<'static, ()> {
        // A poisoned lock means a previous action panicked part-way through.
        // The desktop is in whatever state that left it, which is exactly the
        // state the focus check is there to notice.
        INPUT.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Refuse unless the run is still live and `expected` is still in front.
    ///
    /// `delivered` and `total` go into the error so that a refusal half-way
    /// through a piece of typing says how much of it landed. Pretending nothing
    /// happened would be the more comfortable message and the less true one.
    fn ensure_target(
        &self,
        expected: &str,
        cancel: &CancellationToken,
        delivered: usize,
        total: usize,
    ) -> Result<(), ComputerError> {
        if cancel.is_cancelled() {
            return Err(ComputerError::Cancelled { delivered, total });
        }
        let actual = self.focused()?;
        if actual.name == expected {
            Ok(())
        } else {
            Err(ComputerError::FocusChanged {
                expected: expected.to_owned(),
                actual: actual.name,
                delivered,
                total,
            })
        }
    }
}

#[cfg(target_os = "macos")]
const INPUT_REMEDY: &str = "allow it in System Settings > Privacy & Security > Accessibility";
#[cfg(not(target_os = "macos"))]
const INPUT_REMEDY: &str = "check the operating system's input permissions";

/// Convert an `xcap` failure into ours.
fn xcap_error(operation: &str, error: &xcap::XCapError) -> ComputerError {
    ComputerError::backend(operation, error)
}

fn to_png(image: &xcap::image::RgbaImage, target: String) -> Result<Capture, ComputerError> {
    let mut png = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut png),
            xcap::image::ImageFormat::Png,
        )
        .map_err(|error| ComputerError::backend("encoding the capture", error))?;
    Ok(Capture {
        pixel_width: image.width(),
        pixel_height: image.height(),
        png,
        target,
    })
}

impl Desktop for NativeDesktop {
    fn preflight(&self) -> Preflight {
        #[cfg(target_os = "macos")]
        {
            Preflight {
                supported: true,
                input: grant(macos_accessibility_client::accessibility::application_is_trusted()),
                capture: grant(core_graphics::access::ScreenCaptureAccess.preflight()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Windows gates neither behind a per-application grant. An operator
            // who wants to stop this stops the process.
            Preflight {
                supported: true,
                input: Grant::NotApplicable,
                capture: Grant::NotApplicable,
            }
        }
    }

    fn focused(&self) -> Result<FocusedApplication, ComputerError> {
        let windows =
            xcap::Window::all().map_err(|error| xcap_error("listing the windows", &error))?;
        for window in windows {
            if window.is_focused().unwrap_or(false) {
                return Ok(FocusedApplication {
                    name: window
                        .app_name()
                        .map_err(|error| xcap_error("reading the application name", &error))?,
                    title: window.title().unwrap_or_default(),
                    pid: window.pid().unwrap_or_default(),
                });
            }
        }
        Err(ComputerError::NoFocusedApplication)
    }

    fn windows(&self) -> Result<Vec<WindowInfo>, ComputerError> {
        let windows =
            xcap::Window::all().map_err(|error| xcap_error("listing the windows", &error))?;
        Ok(windows
            .into_iter()
            .map(|window| WindowInfo {
                application: window.app_name().unwrap_or_default(),
                title: window.title().unwrap_or_default(),
                pid: window.pid().unwrap_or_default(),
                origin: Point::new(window.x().unwrap_or(0), window.y().unwrap_or(0)),
                width: window.width().unwrap_or(0),
                height: window.height().unwrap_or(0),
                focused: window.is_focused().unwrap_or(false),
                minimised: window.is_minimized().unwrap_or(false),
            })
            .collect())
    }

    fn displays(&self) -> Result<Vec<DisplayInfo>, ComputerError> {
        let monitors =
            xcap::Monitor::all().map_err(|error| xcap_error("listing the displays", &error))?;
        Ok(monitors
            .into_iter()
            .map(|monitor| DisplayInfo {
                name: monitor.name().unwrap_or_default(),
                origin: Point::new(monitor.x().unwrap_or(0), monitor.y().unwrap_or(0)),
                width: monitor.width().unwrap_or(0),
                height: monitor.height().unwrap_or(0),
                scale: monitor.scale_factor().unwrap_or(1.0),
                primary: monitor.is_primary().unwrap_or(false),
            })
            .collect())
    }

    fn cursor(&self) -> Result<Point, ComputerError> {
        let enigo = Self::enigo()?;
        let (x, y) = enigo
            .location()
            .map_err(|error| ComputerError::backend("reading the cursor position", error))?;
        Ok(Point::new(x, y))
    }

    fn capture_display(&self, name: Option<&str>) -> Result<Capture, ComputerError> {
        let monitors =
            xcap::Monitor::all().map_err(|error| xcap_error("listing the displays", &error))?;
        let monitor = match name {
            Some(wanted) => monitors
                .into_iter()
                .find(|monitor| monitor.name().is_ok_and(|actual| actual == wanted))
                .ok_or_else(|| {
                    ComputerError::backend("finding the display", format!("no display `{wanted}`"))
                })?,
            None => monitors
                .into_iter()
                .find(|monitor| monitor.is_primary().unwrap_or(false))
                .ok_or_else(|| {
                    ComputerError::backend("finding the display", "no primary display")
                })?,
        };
        let label = monitor.name().unwrap_or_else(|_| "display".to_owned());
        let image = monitor
            .capture_image()
            .map_err(|error| xcap_error("capturing the display", &error))?;
        to_png(&image, label)
    }

    fn capture_focused(&self) -> Result<Capture, ComputerError> {
        let windows =
            xcap::Window::all().map_err(|error| xcap_error("listing the windows", &error))?;
        let window = windows
            .into_iter()
            .find(|window| window.is_focused().unwrap_or(false))
            .ok_or(ComputerError::NoFocusedApplication)?;
        let label = window.app_name().unwrap_or_else(|_| "window".to_owned());
        let image = window
            .capture_image()
            .map_err(|error| xcap_error("capturing the window", &error))?;
        to_png(&image, label)
    }

    fn perform(
        &self,
        action: &InputAction,
        expected: &str,
        cancel: &CancellationToken,
    ) -> Result<(), ComputerError> {
        let _guard = Self::lock();
        let mut enigo = Self::enigo()?;
        let total = action.event_count();

        match action {
            InputAction::Move { to } => {
                self.ensure_target(expected, cancel, 0, total)?;
                move_to(&mut enigo, *to)
            }
            InputAction::Click { button, at, count } => {
                if let Some(point) = at {
                    self.ensure_target(expected, cancel, 0, total)?;
                    move_to(&mut enigo, *point)?;
                }
                for delivered in 0..*count {
                    self.ensure_target(expected, cancel, usize::from(delivered), total)?;
                    enigo
                        .button(mouse_button(*button), Direction::Click)
                        .map_err(|error| ComputerError::backend("clicking", error))?;
                }
                Ok(())
            }
            InputAction::Drag { button, from, to } => {
                self.ensure_target(expected, cancel, 0, total)?;
                move_to(&mut enigo, *from)?;
                enigo
                    .button(mouse_button(*button), Direction::Press)
                    .map_err(|error| ComputerError::backend("pressing the button", error))?;
                let moved = self
                    .ensure_target(expected, cancel, 1, total)
                    .and_then(|()| move_to(&mut enigo, *to));
                // Release whatever happened, so a refused drag does not leave a
                // button held down across the whole desktop.
                let released = enigo
                    .button(mouse_button(*button), Direction::Release)
                    .map_err(|error| ComputerError::backend("releasing the button", error));
                moved.and(released)
            }
            InputAction::Scroll { axis, amount } => {
                self.ensure_target(expected, cancel, 0, total)?;
                enigo
                    .scroll(*amount, scroll_axis(*axis))
                    .map_err(|error| ComputerError::backend("scrolling", error))
            }
            InputAction::Type { text } => {
                for (delivered, character) in text.chars().enumerate() {
                    self.ensure_target(expected, cancel, delivered, total)?;
                    if character == '\n' || character == '\r' {
                        enigo
                            .key(enigo::Key::Return, Direction::Click)
                            .map_err(|error| ComputerError::backend("pressing return", error))?;
                    } else {
                        enigo
                            .text(&character.to_string())
                            .map_err(|error| ComputerError::backend("typing", error))?;
                    }
                }
                Ok(())
            }
            InputAction::Key { key, modifiers } => {
                self.ensure_target(expected, cancel, 0, total)?;
                for modifier in modifiers {
                    enigo
                        .key(modifier_key(*modifier), Direction::Press)
                        .map_err(|error| ComputerError::backend("holding a modifier", error))?;
                }
                let pressed = self
                    .ensure_target(expected, cancel, modifiers.len(), total)
                    .and_then(|()| {
                        enigo
                            .key(enigo_key(*key), Direction::Click)
                            .map_err(|error| ComputerError::backend("pressing a key", error))
                    });
                // Modifiers come back up even if the key did not go down. A held
                // Command key outlives the run and breaks the operator's machine.
                let mut released = Ok(());
                for modifier in modifiers.iter().rev() {
                    let result = enigo
                        .key(modifier_key(*modifier), Direction::Release)
                        .map_err(|error| ComputerError::backend("releasing a modifier", error));
                    released = released.and(result);
                }
                pressed.and(released)
            }
        }
    }
}

fn move_to(enigo: &mut Enigo, point: Point) -> Result<(), ComputerError> {
    enigo
        .move_mouse(point.x, point.y, enigo::Coordinate::Abs)
        .map_err(|error| ComputerError::backend("moving the cursor", error))
}

#[cfg(target_os = "macos")]
const fn grant(granted: bool) -> Grant {
    if granted {
        Grant::Granted
    } else {
        Grant::Missing
    }
}

const fn mouse_button(button: Button) -> enigo::Button {
    match button {
        Button::Left => enigo::Button::Left,
        Button::Right => enigo::Button::Right,
        Button::Middle => enigo::Button::Middle,
    }
}

const fn scroll_axis(axis: Axis) -> enigo::Axis {
    match axis {
        Axis::Vertical => enigo::Axis::Vertical,
        Axis::Horizontal => enigo::Axis::Horizontal,
    }
}

const fn modifier_key(modifier: Modifier) -> enigo::Key {
    match modifier {
        Modifier::Shift => enigo::Key::Shift,
        Modifier::Control => enigo::Key::Control,
        Modifier::Alt => enigo::Key::Alt,
        // Command on macOS, the Windows key on Windows — enigo maps this one
        // variant onto each platform's equivalent.
        Modifier::Command => enigo::Key::Meta,
    }
}

/// Map the closed vocabulary onto the platform's key table.
///
/// Every key here exists on both macOS and Windows; that is why the vocabulary
/// is closed. Function keys above 12 and platform-specific keys are absent from
/// [`Key`] rather than silently dropped here.
fn enigo_key(key: Key) -> enigo::Key {
    match key {
        Key::Character(character) => enigo::Key::Unicode(character),
        Key::Escape => enigo::Key::Escape,
        Key::Tab => enigo::Key::Tab,
        Key::Return => enigo::Key::Return,
        Key::Space => enigo::Key::Space,
        Key::Backspace => enigo::Key::Backspace,
        Key::Delete => enigo::Key::Delete,
        Key::Up => enigo::Key::UpArrow,
        Key::Down => enigo::Key::DownArrow,
        Key::Left => enigo::Key::LeftArrow,
        Key::Right => enigo::Key::RightArrow,
        Key::Home => enigo::Key::Home,
        Key::End => enigo::Key::End,
        Key::PageUp => enigo::Key::PageUp,
        Key::PageDown => enigo::Key::PageDown,
        Key::Function(index) => match index {
            1 => enigo::Key::F1,
            2 => enigo::Key::F2,
            3 => enigo::Key::F3,
            4 => enigo::Key::F4,
            5 => enigo::Key::F5,
            6 => enigo::Key::F6,
            7 => enigo::Key::F7,
            8 => enigo::Key::F8,
            9 => enigo::Key::F9,
            10 => enigo::Key::F10,
            11 => enigo::Key::F11,
            // `Key::parse` only produces 1..=12, so this is the twelfth.
            _ => enigo::Key::F12,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_in_the_vocabulary_maps_to_this_platform() {
        // If a key were unmapped the failure would be a wrong keystroke in
        // somebody's terminal, so the whole vocabulary is walked here.
        let keys = [
            Key::Character('a'),
            Key::Escape,
            Key::Tab,
            Key::Return,
            Key::Space,
            Key::Backspace,
            Key::Delete,
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
        ];
        for key in keys {
            let _ = enigo_key(key);
        }
        // Twelve function keys must map to twelve distinct platform keys; a
        // copy-paste slip here would silently send the wrong one.
        let mapped: std::collections::HashSet<_> = (1..=12)
            .map(|index| enigo_key(Key::Function(index)))
            .collect();
        assert_eq!(mapped.len(), 12);
    }
}
