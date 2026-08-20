//! Finding a Chromium to drive.
//!
//! AgentOS does not bundle or download a browser. It looks for one that is
//! already on the machine, in a defined order, and reports clearly when it
//! cannot find one. A tool that silently downloads a hundred megabytes of
//! executable on first use is not something a security-sensitive project should
//! do quietly.

use std::path::{Path, PathBuf};

/// Environment variable that pins the browser executable.
pub const EXECUTABLE_ENV: &str = "AGENTOS_BROWSER";

/// Find a Chromium-family browser to drive.
///
/// Order of preference:
///
/// 1. `explicit` — configured by the operator
/// 2. `AGENTOS_BROWSER`
/// 3. A Playwright-managed Chromium already in the user's cache
/// 4. The platform's usual install locations
///
/// Returns `None` if nothing is found, so the caller can produce an error that
/// says what to install rather than a CDP connection failure.
#[must_use]
pub fn locate(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit
        && is_executable(path)
    {
        return Some(path.to_path_buf());
    }

    if let Some(value) = std::env::var_os(EXECUTABLE_ENV) {
        let path = PathBuf::from(value);
        if is_executable(&path) {
            return Some(path);
        }
    }

    playwright_cache()
        .into_iter()
        .chain(platform_candidates())
        .find(|candidate| is_executable(candidate))
}

/// Human-readable guidance when no browser is found.
#[must_use]
pub fn install_hint() -> String {
    let candidates = platform_candidates()
        .iter()
        .map(|path| format!("  {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "No Chromium-family browser was found. Install Google Chrome, Chromium or Microsoft Edge, \
         or set {EXECUTABLE_ENV} to an executable.\n\nLooked in:\n{candidates}"
    )
}

fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Chromium builds managed by Playwright, which many developers already have.
fn playwright_cache() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };

    #[cfg(target_os = "macos")]
    let root = home.join("Library/Caches/ms-playwright");
    #[cfg(target_os = "windows")]
    let root = home.join("AppData/Local/ms-playwright");
    #[cfg(all(unix, not(target_os = "macos")))]
    let root = home.join(".cache/ms-playwright");

    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };

    // Directory names carry build numbers, so the layout is discovered rather
    // than hard-coded. Headless shell first: it starts faster and has no UI.
    let mut headless = Vec::new();
    let mut full = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("chromium_headless_shell-") {
            headless.extend(descend(&path, HEADLESS_SUFFIXES));
        } else if name.starts_with("chromium-") {
            full.extend(descend(&path, CHROMIUM_SUFFIXES));
        }
    }
    headless.extend(full);
    headless
}

const HEADLESS_SUFFIXES: &[&str] = &[
    "chrome-headless-shell-mac-arm64/chrome-headless-shell",
    "chrome-headless-shell-mac-x64/chrome-headless-shell",
    "chrome-headless-shell-linux/chrome-headless-shell",
    "chrome-headless-shell-win64/chrome-headless-shell.exe",
];

const CHROMIUM_SUFFIXES: &[&str] = &[
    "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    "chrome-mac/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    "chrome-mac-arm64/Chromium.app/Contents/MacOS/Chromium",
    "chrome-mac/Chromium.app/Contents/MacOS/Chromium",
    "chrome-linux/chrome",
    "chrome-win/chrome.exe",
];

fn descend(root: &Path, suffixes: &[&str]) -> Vec<PathBuf> {
    suffixes.iter().map(|suffix| root.join(suffix)).collect()
}

#[cfg(target_os = "macos")]
fn platform_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
        PathBuf::from("/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary"),
        PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
        PathBuf::from("/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"),
    ]
}

#[cfg(target_os = "windows")]
fn platform_candidates() -> Vec<PathBuf> {
    let program_files =
        std::env::var("PROGRAMFILES").unwrap_or_else(|_| r"C:\Program Files".to_owned());
    let program_files_x86 =
        std::env::var("PROGRAMFILES(X86)").unwrap_or_else(|_| r"C:\Program Files (x86)".to_owned());
    vec![
        PathBuf::from(&program_files).join(r"Google\Chrome\Application\chrome.exe"),
        PathBuf::from(&program_files_x86).join(r"Google\Chrome\Application\chrome.exe"),
        PathBuf::from(&program_files).join(r"Microsoft\Edge\Application\msedge.exe"),
        PathBuf::from(&program_files_x86).join(r"Microsoft\Edge\Application\msedge.exe"),
    ]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/bin/google-chrome"),
        PathBuf::from("/usr/bin/google-chrome-stable"),
        PathBuf::from("/usr/bin/chromium"),
        PathBuf::from("/usr/bin/chromium-browser"),
        PathBuf::from("/usr/bin/microsoft-edge"),
        PathBuf::from("/snap/bin/chromium"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_executable_wins() {
        // Any real file will do; the point is that an existing explicit path is
        // preferred over discovery.
        let this_file = PathBuf::from(file!());
        if this_file.is_file() {
            assert_eq!(locate(Some(&this_file)), Some(this_file));
        }
    }

    #[test]
    fn a_missing_explicit_executable_falls_through() {
        // A stale configured path must not stop discovery finding a real one.
        let missing = PathBuf::from("/definitely/not/a/browser");
        assert_ne!(locate(Some(&missing)), Some(missing));
    }

    #[test]
    fn the_hint_names_the_environment_variable_and_the_paths() {
        let hint = install_hint();
        assert!(hint.contains(EXECUTABLE_ENV));
        assert!(hint.contains("Looked in:"));
        assert!(!platform_candidates().is_empty());
    }
}
