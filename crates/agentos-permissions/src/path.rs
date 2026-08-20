//! Secure path resolution.
//!
//! Filesystem scoping is only as good as the function that decides whether a
//! path is inside its scope. Naive prefix matching on the string the caller
//! supplied is not that function: `~/Docs/../../.ssh/id_rsa` starts with
//! `~/Docs`, and so does `~/Docs/link` when `link` points at `/etc`.
//!
//! [`resolve_secure`] resolves a path the way the operating system will,
//! *before* the containment check runs, so both traversal and symlink escapes
//! are caught. Every filesystem-touching tool routes through here; none of them
//! do their own path arithmetic.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::error::PathError;

/// Resolve a path to the location it will actually reach.
///
/// The longest existing prefix is canonicalised — which resolves symlinks,
/// `.` and `..` exactly as the kernel would — and the not-yet-existing
/// remainder is appended. A `..` in the remainder is rejected rather than
/// resolved lexically, because there is nothing to resolve it against.
///
/// The result is absolute and free of symlinks, and is what the containment
/// check in [`is_within`] must be run against.
///
/// # Errors
///
/// Returns [`PathError::NotAbsolute`] for relative input,
/// [`PathError::UnresolvableTraversal`] for `..` past the existing prefix, and
/// [`PathError::Io`] if the filesystem cannot be queried.
pub fn resolve_secure(candidate: &Path) -> Result<PathBuf, PathError> {
    if !candidate.is_absolute() {
        return Err(PathError::NotAbsolute(candidate.to_path_buf()));
    }

    // Peel components off the end until we reach something that exists.
    // `try_exists` is used over `exists` so a permissions error surfaces as an
    // error rather than silently looking like "does not exist".
    //
    // Peeling walks by component rather than by `file_name`, because
    // `Path::file_name` returns `None` for a path ending in `..` — which would
    // abandon the peel exactly on the input a traversal attempt produces.
    let mut existing = candidate.to_path_buf();
    let mut remainder: Vec<OsString> = Vec::new();

    loop {
        match existing.try_exists() {
            Ok(true) => break,
            Ok(false) => {}
            Err(source) => {
                return Err(PathError::Io {
                    path: existing,
                    source,
                });
            }
        }

        match existing.components().next_back() {
            Some(Component::Normal(name)) => remainder.push(name.to_owned()),
            // A `..` that survives into the non-existent tail cannot be resolved
            // against anything real, so resolving it lexically would be a guess.
            // Guessing here is how sandboxes get escaped.
            Some(Component::ParentDir) => {
                return Err(PathError::UnresolvableTraversal(candidate.to_path_buf()));
            }
            Some(Component::CurDir) => {}
            // Reached the root (or a Windows prefix) and it still does not
            // exist. Nothing left to canonicalise against.
            Some(Component::RootDir | Component::Prefix(_)) | None => break,
        }

        if !existing.pop() {
            break;
        }
    }

    let mut resolved = std::fs::canonicalize(&existing).map_err(|source| PathError::Io {
        path: existing.clone(),
        source,
    })?;

    // Every entry here is a `Component::Normal`, guaranteed by the peel above.
    for name in remainder.iter().rev() {
        resolved.push(name);
    }

    Ok(resolved)
}

/// Whether `path` is `root` or lives beneath it.
///
/// Both arguments must already be canonical. Comparison is component-wise, so
/// `/data-private` is correctly *not* inside `/data`.
#[must_use]
pub fn is_within(root: &Path, path: &Path) -> bool {
    path == root || path.starts_with(root)
}

/// Resolve `candidate` and confirm it lands inside one of `roots`.
///
/// This is the function filesystem tools call. It is the single place where
/// "is this path allowed?" is answered.
///
/// # Errors
///
/// Returns [`PathError::OutsideSandbox`] when the resolved path is not under any
/// root, plus anything [`resolve_secure`] can return.
pub fn resolve_within(roots: &[PathBuf], candidate: &Path) -> Result<PathBuf, PathError> {
    let resolved = resolve_secure(candidate)?;
    if roots.iter().any(|root| is_within(root, &resolved)) {
        Ok(resolved)
    } else {
        Err(PathError::OutsideSandbox {
            requested: candidate.to_path_buf(),
            resolved,
        })
    }
}

/// Expand a leading `~` to the user's home directory.
///
/// Only a leading `~` or `~/` is expanded; `~user` is not supported, and a `~`
/// anywhere else is a literal character.
///
/// # Errors
///
/// Returns the input unchanged if there is no `~`; returns `None` if expansion
/// is needed but the home directory cannot be determined.
#[must_use]
pub fn expand_home(path: &str) -> Option<PathBuf> {
    if path == "~" {
        return dirs::home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return dirs::home_dir().map(|home| home.join(rest));
    }
    Some(PathBuf::from(path))
}

/// Number of path components, used as a specificity score.
///
/// A rule scoped to `/a/b/c` is more specific than one scoped to `/a`, so it
/// wins when both match.
#[must_use]
pub fn depth(path: &Path) -> usize {
    path.components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// `TempDir` on macOS hands back `/var/...`, which is a symlink to
    /// `/private/var`. Roots must be canonical or every containment check is
    /// comparing the wrong thing.
    fn canonical_temp() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        (dir, root)
    }

    #[test]
    fn rejects_relative_paths() {
        let err = resolve_secure(Path::new("relative/path")).unwrap_err();
        assert!(matches!(err, PathError::NotAbsolute(_)));
    }

    #[test]
    fn resolves_a_path_that_exists() {
        let (_guard, root) = canonical_temp();
        let file = root.join("a.txt");
        fs::write(&file, "x").unwrap();
        assert_eq!(resolve_secure(&file).unwrap(), file);
    }

    #[test]
    fn resolves_a_path_that_does_not_exist_yet() {
        let (_guard, root) = canonical_temp();
        let target = root.join("nested").join("new.txt");
        assert_eq!(resolve_secure(&target).unwrap(), target);
    }

    #[test]
    fn dot_dot_traversal_escapes_are_caught_by_containment() {
        let (_guard, root) = canonical_temp();
        let inside = root.join("sub");
        fs::create_dir(&inside).unwrap();

        // `/root/sub/../../` climbs out of the sandbox. Resolution reports where
        // it really lands, and containment then rejects it.
        let escape = inside.join("..").join("..");
        let err = resolve_within(std::slice::from_ref(&root), &escape).unwrap_err();
        assert!(matches!(err, PathError::OutsideSandbox { .. }));
    }

    #[test]
    #[cfg(unix)]
    fn symlink_escape_is_blocked() {
        let (_guard, root) = canonical_temp();
        let (_outside_guard, outside) = canonical_temp();
        let secret = outside.join("secret.txt");
        fs::write(&secret, "classified").unwrap();

        let link = root.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        // Textually `link/secret.txt` is inside the sandbox. It is not.
        let err =
            resolve_within(std::slice::from_ref(&root), &link.join("secret.txt")).unwrap_err();
        match err {
            PathError::OutsideSandbox { resolved, .. } => assert_eq!(resolved, secret),
            other => panic!("expected sandbox escape, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn symlink_escape_is_blocked_for_files_that_do_not_exist_yet() {
        let (_guard, root) = canonical_temp();
        let (_outside_guard, outside) = canonical_temp();

        let link = root.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        // Writing a *new* file through the link must be caught too — this is the
        // case a "does the file exist?" check would miss.
        let err =
            resolve_within(std::slice::from_ref(&root), &link.join("planted.txt")).unwrap_err();
        assert!(matches!(err, PathError::OutsideSandbox { .. }));
    }

    #[test]
    fn dot_dot_past_the_existing_prefix_is_rejected() {
        let (_guard, root) = canonical_temp();
        let err = resolve_secure(&root.join("nope").join("..").join("..")).unwrap_err();
        assert!(matches!(err, PathError::UnresolvableTraversal(_)));
    }

    #[test]
    fn paths_inside_the_sandbox_are_allowed() {
        let (_guard, root) = canonical_temp();
        let file = root.join("deep").join("deeper").join("f.txt");
        assert_eq!(
            resolve_within(std::slice::from_ref(&root), &file).unwrap(),
            file
        );
    }

    #[test]
    fn sibling_prefixes_do_not_count_as_inside() {
        // `/data-private` must not be considered inside `/data`.
        assert!(!is_within(Path::new("/data"), Path::new("/data-private")));
        assert!(is_within(Path::new("/data"), Path::new("/data/x")));
        assert!(is_within(Path::new("/data"), Path::new("/data")));
    }

    #[test]
    fn multiple_roots_are_each_considered() {
        let (_a_guard, a) = canonical_temp();
        let (_b_guard, b) = canonical_temp();
        let target = b.join("f.txt");
        assert_eq!(resolve_within(&[a, b.clone()], &target).unwrap(), target);
    }

    #[test]
    fn traversal_in_a_non_existent_tail_is_rejected() {
        let (_guard, root) = canonical_temp();
        // Nothing under `nope` exists, so there is no real directory for `..`
        // to be resolved against.
        for candidate in [
            root.join("nope").join("..").join(".."),
            root.join("nope").join("..").join("etc").join("passwd"),
        ] {
            let err = resolve_secure(&candidate).unwrap_err();
            assert!(
                matches!(err, PathError::UnresolvableTraversal(_)),
                "expected traversal rejection for {}, got {err:?}",
                candidate.display()
            );
        }
    }

    #[test]
    fn traversal_through_an_existing_directory_resolves_then_fails_containment() {
        let (_guard, root) = canonical_temp();
        let sub = root.join("sub");
        fs::create_dir(&sub).unwrap();
        // `sub/../elsewhere.txt` is `root/elsewhere.txt`, which is outside a
        // sandbox rooted at `sub`.
        let outside = root.join("elsewhere.txt");

        // `sub` exists, so the kernel can resolve the `..`; the result simply
        // lands outside the sandbox and containment rejects it.
        let err = resolve_within(
            std::slice::from_ref(&sub),
            &sub.join("..").join("elsewhere.txt"),
        )
        .unwrap_err();
        match err {
            PathError::OutsideSandbox { resolved, .. } => assert_eq!(resolved, outside),
            other => panic!("expected sandbox escape, got {other:?}"),
        }
    }

    /// An absolute path that does not exist, spelled for the host platform.
    ///
    /// `/foo` is not absolute on Windows — it has no drive prefix — so a
    /// hard-coded Unix path would test argument validation rather than
    /// resolution there.
    fn missing_absolute_path() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\definitely\not\here\at\all")
        } else {
            PathBuf::from("/definitely/not/here/at/all")
        }
    }

    #[test]
    fn a_wholly_missing_path_still_resolves_but_fails_containment() {
        // Resolution is not an existence check: an agent creating a new file
        // must get a resolved path back. Containment is what rejects it.
        let target = missing_absolute_path();
        assert_eq!(resolve_secure(&target).unwrap(), target);

        let (_guard, root) = canonical_temp();
        let err = resolve_within(&[root], &target).unwrap_err();
        assert!(matches!(err, PathError::OutsideSandbox { .. }));
    }

    #[test]
    fn depth_counts_normal_components() {
        assert_eq!(depth(Path::new("/a/b/c")), 3);
        assert_eq!(depth(Path::new("/")), 0);
    }

    #[test]
    fn expand_home_leaves_ordinary_paths_alone() {
        assert_eq!(expand_home("/usr/local"), Some(PathBuf::from("/usr/local")));
    }
}
