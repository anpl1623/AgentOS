//! Choosing a backend.
//!
//! macOS and Windows have one. Everywhere else the crate still compiles and
//! every tool refuses with [`ComputerError::Unsupported`](crate::ComputerError::Unsupported), which keeps the
//! authorisation logic — the part worth testing — running on every platform in
//! CI without dragging an X11 or Wayland stack into the build.

use std::sync::Arc;

use crate::desktop::Desktop;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod unsupported;

/// The backend for this platform.
#[must_use]
pub fn current() -> Arc<dyn Desktop> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        Arc::new(native::NativeDesktop::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Arc::new(unsupported::UnsupportedDesktop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backend_exists_and_reports_whether_it_works() {
        let desktop = current();
        let preflight = desktop.preflight();
        assert_eq!(
            preflight.supported,
            cfg!(any(target_os = "macos", target_os = "windows"))
        );
    }
}
