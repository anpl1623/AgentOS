//! The checks every argument passes before anything is planned.
//!
//! What reaches a path is held to an alphabet that cannot leave it, and what
//! is shown to a person and posted is held to GitHub's own limits, so that an
//! approval is never given for a request GitHub would refuse or that would go
//! somewhere other than the card says.

use agentos_core::permission::{Capability, ResourceRef};

use super::args::MAX_LIMIT;
use crate::account::is_label;
use crate::github::INTEGRATION;

/// GitHub's own limits, enforced before anything is planned so an approval is
/// never given for a request GitHub would refuse for its size.
pub(super) const MAX_TITLE_CHARS: usize = 256;
const MAX_BODY_CHARS: usize = 65_536;
const MAX_LABEL_CHARS: usize = 50;
const MAX_LABELS: usize = 100;
const MAX_REF_BYTES: usize = 255;
const MAX_PATH_BYTES: usize = 1024;
const MAX_REPO_PART_BYTES: usize = 100;

/// A repository, as validated.
///
/// Lower-cased, because GitHub's names are case-insensitive and policy globs
/// are not: were `Acme/Secret` left as written, a deny of `acme/secret` would
/// not bind it while GitHub served the same repository. Rules are therefore
/// written, and matched, in lower case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    owner: String,
    name: String,
}

impl Repo {
    /// Parse `owner/name`.
    ///
    /// Each half is 1 to 100 of `A-Z a-z 0-9 . _ -`, and is not made of dots
    /// alone. That last rule is not GitHub's tidiness: the halves become path
    /// segments, and a URL parser reads `..` as "go up", so `../user` would
    /// leave `/repos/` for an endpoint the capability does not describe.
    ///
    /// # Errors
    ///
    /// A message saying what is wrong.
    pub fn parse(repo: &str) -> Result<Self, String> {
        let malformed = || {
            format!(
                "`{}` is not a repository: write `owner/name`, each 1 to \
                 {MAX_REPO_PART_BYTES} letters, digits, `.`, `_` or `-`",
                repo.escape_debug()
            )
        };
        let (owner, name) = repo.split_once('/').ok_or_else(malformed)?;
        for part in [owner, name] {
            let allowed = |byte: u8| byte.is_ascii_alphanumeric() || b"._-".contains(&byte);
            if part.is_empty()
                || part.len() > MAX_REPO_PART_BYTES
                || !part.bytes().all(allowed)
                || part.bytes().all(|byte| byte == b'.')
            {
                return Err(malformed());
            }
        }
        Ok(Self {
            owner: owner.to_ascii_lowercase(),
            name: name.to_ascii_lowercase(),
        })
    }

    /// The API path of the repository itself, `/repos/owner/name`.
    #[must_use]
    pub fn path(&self) -> String {
        format!("/repos/{}/{}", self.owner, self.name)
    }

    /// The capability `action` needs on this repository.
    #[must_use]
    pub fn capability(&self, action: &str) -> Capability {
        Capability::new(INTEGRATION, action).with_resource(ResourceRef::Named {
            name: self.to_string(),
        })
    }
}

impl std::fmt::Display for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// A check failed while validating arguments, before the tool name is known.
type Check = Result<(), String>;

pub(super) fn check_number(number: u64) -> Check {
    if number == 0 {
        return Err("`number` starts at 1".into());
    }
    Ok(())
}

pub(super) fn check_limit(limit: u8) -> Check {
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(format!("`limit` must be 1 to {MAX_LIMIT}"));
    }
    Ok(())
}

pub(super) fn check_account(account: Option<&str>) -> Check {
    match account {
        Some(label) if !is_label(label) => Err(format!(
            "`{}` is not an account label: labels are lower-case letters, digits and `-`",
            label.escape_debug()
        )),
        _ => Ok(()),
    }
}

/// Text that is shown to a person and sent as written: no control characters
/// but line breaks and tabs, and no longer than GitHub accepts.
fn check_text(field: &str, text: &str, max_chars: usize, allow_breaks: bool) -> Check {
    if text.chars().count() > max_chars {
        return Err(format!("`{field}` is longer than {max_chars} characters"));
    }
    let bad = |c: char| c.is_control() && !(allow_breaks && matches!(c, '\n' | '\r' | '\t'));
    if text.chars().any(bad) {
        return Err(format!("`{field}` contains a control character"));
    }
    Ok(())
}

pub(super) fn check_title(title: &str) -> Check {
    if title.trim().is_empty() {
        return Err("`title` is empty".into());
    }
    check_text("title", title, MAX_TITLE_CHARS, false)
}

pub(super) fn check_body(body: &str, required: bool) -> Check {
    if required && body.trim().is_empty() {
        return Err("`body` is empty".into());
    }
    check_text("body", body, MAX_BODY_CHARS, true)
}

/// Labels. A filter's labels are joined with commas into one query value, so
/// there a comma would split one label into two.
pub(super) fn check_labels(labels: &[String], filter: bool) -> Check {
    if labels.len() > MAX_LABELS {
        return Err(format!("at most {MAX_LABELS} labels"));
    }
    for label in labels {
        if label.trim().is_empty() {
            return Err("a label is empty".into());
        }
        check_text("labels", label, MAX_LABEL_CHARS, false)?;
        if filter && label.contains(',') {
            return Err(format!(
                "label `{}` contains a comma, which a filter cannot carry",
                label.escape_debug()
            ));
        }
    }
    Ok(())
}

/// A branch, tag or commit, as git would accept it.
///
/// Stricter than it needs to be for a JSON field, and exactly as strict as it
/// needs to be for a path segment: no `..`, no segment of dots, nothing a URL
/// parser or git treats as syntax. A ref that reaches a path is also
/// percent-encoded on the way (see [`super::api`]), so `/` in a branch name
/// stays inside the segment.
///
/// [`super::api`]: crate::github::api
pub(super) fn check_ref(field: &str, value: &str, allow_fork: bool) -> Check {
    let refuse = || {
        Err(format!(
            "`{}` is not a {field}: give a branch, tag or commit as git names it",
            value.escape_debug()
        ))
    };
    let value = if allow_fork {
        // `user:branch` names a branch in a fork; the user half is a GitHub
        // login, the branch half a ref like any other.
        match value.split_once(':') {
            Some((user, branch)) => {
                let login = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'-';
                if user.is_empty() || !user.bytes().all(login) {
                    return refuse();
                }
                branch
            }
            None => value,
        }
    } else {
        value
    };
    let forbidden = |c: char| {
        c.is_control() || c.is_whitespace() || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    };
    if value.is_empty()
        || value.len() > MAX_REF_BYTES
        || value.contains("..")
        || value.contains("//")
        || value.contains("@{")
        || value.starts_with(['/', '-', '.'])
        || value.ends_with(['/', '.'])
        || value.ends_with(".lock")
        || value.chars().any(forbidden)
        || value.split('/').any(|segment| segment.starts_with('.'))
    {
        return refuse();
    }
    Ok(())
}

pub(super) fn check_path(path: &str) -> Check {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path.chars().any(char::is_control)
    {
        return Err(format!(
            "`{}` is not a file path in the repository",
            path.escape_debug()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_is_owner_and_name_and_nothing_a_path_could_climb_out_of() {
        assert_eq!(
            Repo::parse("Acme/Widgets").unwrap().to_string(),
            "acme/widgets"
        );
        assert_eq!(
            Repo::parse("a.b_c-d/x.y").unwrap().path(),
            "/repos/a.b_c-d/x.y"
        );
        for bad in [
            "",
            "acme",
            "acme/",
            "/widgets",
            "acme/widgets/issues",
            "../user",
            "acme/..",
            "acme/.",
            "./x",
            "acme/wid gets",
            "acme/widgets?x=1",
            "acme/widgets#1",
            "acme/%2e%2e",
            "acme@evil.example/x",
            "acme/wïdgets",
            &format!("{}/x", "a".repeat(MAX_REPO_PART_BYTES + 1)),
        ] {
            assert!(Repo::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn refs_that_a_path_or_git_would_misread_are_refused() {
        for good in ["main", "release/0.2", "feature/x-y_z", "v1.2.3", "a1b2c3d"] {
            assert!(check_ref("git ref", good, false).is_ok(), "{good}");
        }
        assert!(check_ref("head", "octo:fix/x", true).is_ok());
        for bad in [
            "", "..", ".", "a/../b", "a/./b", "/main", "main/", "-x", "a b", "a:b", "a~1", "a^",
            "a?", "a*", "a[", "a\\b", "a//b", "x.lock", "a@{1}", "a\nb",
        ] {
            assert!(check_ref("git ref", bad, false).is_err(), "{bad:?}");
        }
        assert!(check_ref("head", "octo:", true).is_err());
        assert!(check_ref("head", "oc to:x", true).is_err());
        assert!(check_ref("base", "octo:x", false).is_err());
    }
}
