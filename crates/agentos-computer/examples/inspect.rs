//! Read this machine's desktop through the real backend.
//!
//! ```bash
//! cargo run -p agentos-computer --example inspect
//! ```
//!
//! Deliberately read-only: it reports what the backend can see, and sends no
//! input. Checking that the tools work should not involve taking the mouse away
//! from whoever is running it.
#![allow(clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)]

use agentos_computer::Grant;

fn main() {
    let desktop = agentos_computer::current();
    let preflight = desktop.preflight();

    println!("supported here: {}", preflight.supported);
    println!("input grant:    {:?}", preflight.input);
    println!("capture grant:  {:?}", preflight.capture);
    if !preflight.supported {
        return;
    }
    if preflight.input.blocks() || preflight.capture.blocks() {
        println!("\nA grant is missing — run `agentos doctor` for where to give it.");
    }

    match desktop.displays() {
        Ok(displays) => {
            for display in displays {
                println!(
                    "\ndisplay {} — {}x{} points at {}, scale {}{}",
                    display.name,
                    display.width,
                    display.height,
                    display.origin,
                    display.scale,
                    if display.primary { " (primary)" } else { "" }
                );
            }
        }
        Err(error) => println!("\ndisplays: {error}"),
    }

    match desktop.cursor() {
        Ok(point) => println!("cursor at {point}"),
        Err(error) => println!("cursor: {error}"),
    }

    match desktop.focused() {
        Ok(application) => println!("in front: {} (pid {})", application.name, application.pid),
        Err(error) => println!("in front: {error}"),
    }

    // Window titles are somebody else's text, which is why reading them raises
    // taint. Only the count is printed here, for the same reason.
    match desktop.windows() {
        Ok(windows) => println!("{} window(s) visible", windows.len()),
        Err(error) => println!("windows: {error}"),
    }

    if matches!(preflight.capture, Grant::Granted | Grant::NotApplicable) {
        match desktop.capture_focused() {
            Ok(capture) => println!(
                "captured {} at {}x{} pixels ({} bytes of PNG)",
                capture.target,
                capture.pixel_width,
                capture.pixel_height,
                capture.png.len()
            ),
            Err(error) => println!("capture: {error}"),
        }
    }
}
