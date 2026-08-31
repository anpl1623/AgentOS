//! The backend for platforms AgentOS does not drive.
//!
//! It refuses everything. That is not a stub: a tool that is registered,
//! authorised, and then honestly reports that this machine cannot do it is
//! better than one that is missing, because the operator gets an explanation
//! instead of a tool name that does not exist.

use crate::desktop::{Capture, Desktop, DisplayInfo, FocusedApplication, Preflight, WindowInfo};
use crate::error::ComputerError;
use crate::input::{InputAction, Point};
use tokio_util::sync::CancellationToken;

/// A desktop that cannot be reached from here.
#[derive(Debug)]
pub(crate) struct UnsupportedDesktop;

impl Desktop for UnsupportedDesktop {
    fn preflight(&self) -> Preflight {
        Preflight::unsupported()
    }

    fn focused(&self) -> Result<FocusedApplication, ComputerError> {
        Err(ComputerError::Unsupported)
    }

    fn windows(&self) -> Result<Vec<WindowInfo>, ComputerError> {
        Err(ComputerError::Unsupported)
    }

    fn displays(&self) -> Result<Vec<DisplayInfo>, ComputerError> {
        Err(ComputerError::Unsupported)
    }

    fn cursor(&self) -> Result<Point, ComputerError> {
        Err(ComputerError::Unsupported)
    }

    fn capture_display(&self, _name: Option<&str>) -> Result<Capture, ComputerError> {
        Err(ComputerError::Unsupported)
    }

    fn capture_focused(&self) -> Result<Capture, ComputerError> {
        Err(ComputerError::Unsupported)
    }

    fn perform(
        &self,
        _action: &InputAction,
        _expected: &str,
        _cancel: &CancellationToken,
    ) -> Result<(), ComputerError> {
        Err(ComputerError::Unsupported)
    }
}
