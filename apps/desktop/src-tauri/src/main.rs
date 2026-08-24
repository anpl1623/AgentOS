//! The AgentOS desktop binary.
//!
//! Everything lives in the library so the command surface can be tested without
//! a window.

// Do not open a console window alongside the application on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    agentos_desktop_lib::run();
}
