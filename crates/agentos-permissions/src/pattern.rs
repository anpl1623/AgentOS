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

/// Which sort of thing a resource is.
///
/// Exists so that comparing a pattern against a resource is a comparison of
/// kinds first and values second, and so that adding a [`ResourceRef`] variant
/// without teaching the matcher about it fails to compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceKind {
    Path,
    Program,
    Origin,
    Application,
    Named,
}

impl ResourceKind {
    const fn of(resource: &ResourceRef) -> Self {
        match resource {
            ResourceRef::Path { .. } => Self::Path,
            ResourceRef::Program { .. } => Self::Program,
            ResourceRef::Origin { .. } => Self::Origin,
            ResourceRef::Application { .. } => Self::Application,
            ResourceRef::Named { .. } => Self::Named,
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
    /// Matches a desktop application by glob.
    Application {
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
            GlobKind::Application => Self::Application { source, matcher },
            GlobKind::Named => Self::Named { source, matcher },
        })
    }

    /// A path-prefix pattern. The root must already be canonical.
    #[must_use]
    pub const fn path_prefix(root: PathBuf) -> Self {
        Self::PathPrefix { root }
    }

    /// The kind of resource this pattern is written against.
    ///
    /// `None` for [`Self::Any`], which is not about any one kind.
    const fn kind(&self) -> Option<ResourceKind> {
        match self {
            Self::Any => None,
            Self::PathPrefix { .. } => Some(ResourceKind::Path),
            Self::Program { .. } => Some(ResourceKind::Program),
            Self::Origin { .. } => Some(ResourceKind::Origin),
            Self::Application { .. } => Some(ResourceKind::Application),
            Self::Named { .. } => Some(ResourceKind::Named),
        }
    }

    /// Whether this pattern matches `resource`.
    ///
    /// A capability with no resource is matched only by [`Self::Any`]: a rule
    /// scoped to specific paths must not silently apply to an unscoped request.
    ///
    /// Kinds are compared before values, through a private `ResourceKind`, so
    /// that a new resource kind is a compile error here — in the function that
    /// decides authorisation — rather than a silent refusal to match.
    #[must_use]
    pub fn matches(&self, resource: Option<&ResourceRef>) -> bool {
        let Some(resource) = resource else {
            return matches!(self, Self::Any);
        };
        // A pattern of one kind never matches a resource of another.
        if self
            .kind()
            .is_some_and(|kind| kind != ResourceKind::of(resource))
        {
            return false;
        }
        match (self, resource) {
            (Self::Any, _) => true,
            (Self::PathPrefix { root }, ResourceRef::Path { path }) => {
                is_within(root, Path::new(path))
            }
            (Self::Program { matcher, .. }, ResourceRef::Program { program }) => {
                matcher.is_match(program)
            }
            (Self::Origin { matcher, .. }, ResourceRef::Origin { origin }) => {
                matcher.is_match(origin)
            }
            (Self::Application { matcher, .. }, ResourceRef::Application { application }) => {
                matcher.is_match(application)
            }
            (Self::Named { matcher, .. }, ResourceRef::Named { name }) => matcher.is_match(name),
            // Unreachable: the kinds agreed, so the pair is one of the above.
            // Refusing is the right answer if that ever stops being true.
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
            | Self::Application { source, .. }
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
            Self::Application { source, .. } => format!("application:{source}"),
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
    /// A desktop application.
    Application,
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

        // An application and an integration account can share a name. A rule
        // about one must not authorise the other.
        let application = ResourcePattern::glob(GlobKind::Application, "Mail").unwrap();
        let named = ResourcePattern::glob(GlobKind::Named, "Mail").unwrap();
        assert!(application.matches(Some(&ResourceRef::Application {
            application: "Mail".into()
        })));
        assert!(!application.matches(Some(&ResourceRef::Named {
            name: "Mail".into()
        })));
        assert!(!named.matches(Some(&ResourceRef::Application {
            application: "Mail".into()
        })));
        assert_ne!(application, named);
    }

    #[test]
    fn application_globs_match_by_name() {
        let pattern = ResourcePattern::glob(GlobKind::Application, "Mail").unwrap();
        assert!(pattern.matches(Some(&ResourceRef::Application {
            application: "Mail".into()
        })));
        // Matching is case-sensitive and anchored, so a lookalike is refused.
        assert!(!pattern.matches(Some(&ResourceRef::Application {
            application: "Mail Stealer".into()
        })));
        assert!(!pattern.matches(Some(&ResourceRef::Application {
            application: "mail".into()
        })));
        assert_eq!(pattern.describe(), "application:Mail");
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
