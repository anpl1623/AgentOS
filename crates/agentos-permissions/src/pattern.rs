//! Matching rules against capabilities.
//!
//! Patterns are deliberately boring: exact strings, `*`, and globs. Specificity
//! is a comparable score so that "the most specific rule wins" is a total order
//! rather than a heuristic, and conflicts resolve identically every time.

use std::path::{Path, PathBuf};

use agentos_core::permission::ResourceRef;
use globset::{Glob, GlobMatcher};

use crate::path::{depth, is_within};

/// Matches the `domain` or `action` half of a capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamePattern {
    /// Matches anything.
    Any,
    /// Matches one exact string.
    Exact(String),
}

impl NamePattern {
    /// Parse `*` as [`Self::Any`] and anything else as an exact match.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        if raw == "*" {
            Self::Any
        } else {
            Self::Exact(raw.to_owned())
        }
    }

    /// Whether this pattern matches `value`.
    #[must_use]
    pub fn matches(&self, value: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => expected == value,
        }
    }

    /// Specificity contribution: an exact match outranks a wildcard.
    #[must_use]
    pub const fn specificity(&self) -> u32 {
        match self {
            Self::Any => 0,
            Self::Exact(_) => 1,
        }
    }
}

/// Matches the resource half of a capability.
#[derive(Debug, Clone)]
pub enum ResourcePattern {
    /// Matches any resource, including capabilities that have none.
    Any,
    /// Matches a filesystem path at or beneath a canonical root.
    PathPrefix {
        /// The canonical root.
        root: PathBuf,
    },
    /// Matches a program name or path by glob.
    Program {
        /// Source text, retained for audit messages.
        source: String,
        /// Compiled matcher.
        matcher: GlobMatcher,
    },
    /// Matches a network origin by glob.
    Origin {
        /// Source text.
        source: String,
        /// Compiled matcher.
        matcher: GlobMatcher,
    },
    /// Matches a named resource by glob.
    Named {
        /// Source text.
        source: String,
        /// Compiled matcher.
        matcher: GlobMatcher,
    },
}

impl PartialEq for ResourcePattern {
    fn eq(&self, other: &Self) -> bool {
        self.describe() == other.describe()
    }
}

impl ResourcePattern {
    /// Compile a glob-based pattern.
    ///
    /// # Errors
    ///
    /// Returns the underlying [`globset::Error`] if the glob is malformed.
    pub fn glob(kind: GlobKind, source: &str) -> Result<Self, globset::Error> {
        let matcher = Glob::new(source)?.compile_matcher();
        let source = source.to_owned();
        Ok(match kind {
            GlobKind::Program => Self::Program { source, matcher },
            GlobKind::Origin => Self::Origin { source, matcher },
            GlobKind::Named => Self::Named { source, matcher },
        })
    }

    /// A path-prefix pattern. The root must already be canonical.
    #[must_use]
    pub const fn path_prefix(root: PathBuf) -> Self {
        Self::PathPrefix { root }
    }

    /// Whether this pattern matches `resource`.
    ///
    /// A capability with no resource is matched only by [`Self::Any`]: a rule
    /// scoped to specific paths must not silently apply to an unscoped request.
    #[must_use]
    pub fn matches(&self, resource: Option<&ResourceRef>) -> bool {
        match (self, resource) {
            (Self::Any, _) => true,
            (_, None) => false,
            (Self::PathPrefix { root }, Some(ResourceRef::Path { path })) => {
                is_within(root, Path::new(path))
            }
            (Self::Program { matcher, .. }, Some(ResourceRef::Program { program })) => {
                matcher.is_match(program)
            }
            (Self::Origin { matcher, .. }, Some(ResourceRef::Origin { origin })) => {
                matcher.is_match(origin)
            }
            (Self::Named { matcher, .. }, Some(ResourceRef::Named { name })) => {
                matcher.is_match(name)
            }
            // A pattern of one kind never matches a resource of another.
            _ => false,
        }
    }

    /// Specificity contribution.
    ///
    /// Deeper roots and longer literal globs outrank shallower and broader ones.
    #[must_use]
    pub fn specificity(&self) -> u32 {
        match self {
            Self::Any => 0,
            Self::PathPrefix { root } => {
                // +1 so that a root of `/` still outranks `Any`.
                u32::try_from(depth(root))
                    .unwrap_or(u32::MAX)
                    .saturating_add(1)
            }
            Self::Program { source, .. }
            | Self::Origin { source, .. }
            | Self::Named { source, .. } => {
                if source == "*" {
                    1
                } else {
                    u32::try_from(source.chars().filter(|c| *c != '*').count())
                        .unwrap_or(u32::MAX)
                        .saturating_add(1)
                }
            }
        }
    }

    /// Human-readable form for audit messages.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Any => "*".to_owned(),
            Self::PathPrefix { root } => format!("path:{}", root.display()),
            Self::Program { source, .. } => format!("program:{source}"),
            Self::Origin { source, .. } => format!("origin:{source}"),
            Self::Named { source, .. } => format!("name:{source}"),
        }
    }
}

/// Which kind of glob pattern to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobKind {
    /// A program name or path.
    Program,
    /// A network origin.
    Origin,
    /// A named resource.
    Named,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_name_matches_anything() {
        assert!(NamePattern::parse("*").matches("filesystem"));
        assert!(NamePattern::parse("filesystem").matches("filesystem"));
        assert!(!NamePattern::parse("filesystem").matches("terminal"));
    }

    #[test]
    fn exact_names_are_more_specific_than_wildcards() {
        assert!(
            NamePattern::parse("filesystem").specificity() > NamePattern::parse("*").specificity()
        );
    }

    #[test]
    fn path_patterns_match_descendants_only() {
        let pattern = ResourcePattern::path_prefix(PathBuf::from("/home/u/Docs"));
        assert!(pattern.matches(Some(&ResourceRef::Path {
            path: "/home/u/Docs/a.txt".into()
        })));
        assert!(pattern.matches(Some(&ResourceRef::Path {
            path: "/home/u/Docs".into()
        })));
        assert!(!pattern.matches(Some(&ResourceRef::Path {
            path: "/home/u/Documents/a.txt".into()
        })));
        assert!(!pattern.matches(Some(&ResourceRef::Path {
            path: "/etc/passwd".into()
        })));
    }

    #[test]
    fn deeper_roots_are_more_specific() {
        let shallow = ResourcePattern::path_prefix(PathBuf::from("/home"));
        let deep = ResourcePattern::path_prefix(PathBuf::from("/home/u/Docs/Sales"));
        assert!(deep.specificity() > shallow.specificity());
        assert!(shallow.specificity() > ResourcePattern::Any.specificity());
    }

    #[test]
    fn patterns_do_not_match_across_resource_kinds() {
        let pattern = ResourcePattern::path_prefix(PathBuf::from("/home"));
        assert!(!pattern.matches(Some(&ResourceRef::Program {
            program: "/home/bin/x".into()
        })));
    }

    #[test]
    fn scoped_patterns_do_not_match_unscoped_capabilities() {
        // Otherwise a rule saying "allow writes under ~/Sales" would also allow
        // a write with no path attached.
        let pattern = ResourcePattern::path_prefix(PathBuf::from("/home"));
        assert!(!pattern.matches(None));
        assert!(ResourcePattern::Any.matches(None));
    }

    #[test]
    fn program_globs_match() {
        let pattern = ResourcePattern::glob(GlobKind::Program, "git").unwrap();
        assert!(pattern.matches(Some(&ResourceRef::Program {
            program: "git".into()
        })));
        assert!(!pattern.matches(Some(&ResourceRef::Program {
            program: "rm".into()
        })));
    }

    #[test]
    fn origin_globs_support_wildcards() {
        let pattern = ResourcePattern::glob(GlobKind::Origin, "https://*.example.com").unwrap();
        assert!(pattern.matches(Some(&ResourceRef::Origin {
            origin: "https://crm.example.com".into()
        })));
        assert!(!pattern.matches(Some(&ResourceRef::Origin {
            origin: "https://evil.com".into()
        })));
    }

    #[test]
    fn literal_globs_outrank_wildcards() {
        let literal = ResourcePattern::glob(GlobKind::Program, "git").unwrap();
        let wildcard = ResourcePattern::glob(GlobKind::Program, "*").unwrap();
        assert!(literal.specificity() > wildcard.specificity());
        assert!(wildcard.specificity() > ResourcePattern::Any.specificity());
    }
}
