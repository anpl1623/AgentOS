//! Network origins, reduced to one spelling.
//!
//! The origin — `scheme://host[:port]` — is the unit of network policy. It is
//! only a unit if every way of writing the same server reduces to the same
//! string before a rule is compared against it: `https://API.Example.com:443`
//! and `https://api.example.com` are one server, and a policy glob is
//! case-sensitive and knows nothing about default ports. Left to string
//! comparison they are two subjects, and a rule written against one does not
//! bind the other.
//!
//! Both sides are therefore canonicalised. [`normalise_origin`] reduces the URL
//! a tool is about to act on; the policy compiler reduces the origin patterns an
//! operator wrote, and refuses the ones it cannot read rather than compiling a
//! rule that matches nothing.
//!
//! A pattern is then matched part by part — scheme, host, port — rather than as
//! one string, so that a host wildcard cannot run on into the port. A pattern
//! that names no port means the scheme's default port, whether its host is
//! literal or a glob: `https://*.example.com` and `https://admin.example.com`
//! both bind port 443 and nothing else, and `:*` is how any port is written.
//! Were a host glob allowed to absorb the port, an allow of `https://*` would
//! admit `https://evil.example:8443` past a deny of `https://evil.example`.
//!
//! The parser is deliberately narrower than a browser's. Anything a browser
//! would rewrite into a different host — percent-escapes, non-ASCII, the
//! decimal and hexadecimal spellings of an IPv4 address, an IPv4 address
//! written as IPv6 — is refused rather than reinterpreted, since the one outcome
//! that must never happen is the policy seeing one host while the browser
//! visits another. The unspecified addresses, `0.0.0.0` and `[::]`, are refused
//! as well: they are nobody's server, and connecting to one reaches whatever is
//! listening locally.

use std::net::{Ipv4Addr, Ipv6Addr};

use globset::{Glob, GlobMatcher};

use crate::error::{OriginError, PatternError};

/// Reduce a URL to its canonical origin, `scheme://host[:port]`.
///
/// The scheme and host are lower-cased, the port is dropped when it is the
/// scheme's default, IPv6 literals are rewritten to their compressed form, and
/// a single trailing dot on the host is removed. Path, query and fragment are
/// discarded: granting an agent one page of a site and not another is a
/// distinction browsers do not enforce and neither should a policy pretend to.
///
/// # Errors
///
/// [`OriginError::UnsupportedScheme`] for anything but `http` and `https`,
/// including input with no scheme at all; [`OriginError::NoHost`] for an empty
/// authority; [`OriginError::CredentialsInUrl`] for userinfo;
/// [`OriginError::InvalidHost`] for a host outside the plain ASCII form; and
/// [`OriginError::InvalidPort`] for a port that is not 1-65535.
pub fn normalise_origin(url: &str) -> Result<String, OriginError> {
    Origin::parse(url).map(|origin| origin.canonical())
}

/// A parsed origin. `port` is `None` when it is the scheme default.
#[derive(Debug)]
pub(crate) struct Origin {
    scheme: &'static str,
    host: String,
    port: Option<u16>,
}

impl Origin {
    /// Parse and canonicalise, as [`normalise_origin`] documents.
    pub(crate) fn parse(url: &str) -> Result<Self, OriginError> {
        let unsupported = || OriginError::UnsupportedScheme {
            url: url.to_owned(),
        };
        let (scheme, rest) = url.split_once("://").ok_or_else(unsupported)?;
        let scheme = web_scheme(scheme).ok_or_else(unsupported)?;

        // A browser treats `\` as `/` in an http(s) URL, so it ends the
        // authority here too. Otherwise `https://evil.example\x.trusted.example`
        // would be matched as a subdomain of trusted.example while the browser
        // went to evil.example.
        let authority = rest.split(['/', '\\', '?', '#']).next().unwrap_or_default();
        if authority.is_empty() {
            return Err(OriginError::NoHost {
                url: url.to_owned(),
            });
        }
        // Credentials in a URL are not something an agent should be
        // constructing, and they make the origin ambiguous to a human reader:
        // `https://trusted.example@evil.example` goes to evil.example.
        if authority.contains('@') {
            return Err(OriginError::CredentialsInUrl {
                url: url.to_owned(),
            });
        }

        let invalid_host = || OriginError::InvalidHost {
            url: url.to_owned(),
        };
        let invalid_port = || OriginError::InvalidPort {
            url: url.to_owned(),
        };
        let (host, port) = split_authority(authority).ok_or_else(invalid_host)?;
        if host.is_empty() {
            // Only reachable as `https://:8080`: an authority that is all port.
            return Err(invalid_port());
        }
        let host = literal_host(host).ok_or_else(invalid_host)?;
        let port = match port {
            None => None,
            Some(raw) => {
                let port = parse_port(raw).ok_or_else(invalid_port)?;
                (port != default_port(scheme)).then_some(port)
            }
        };
        Ok(Self { scheme, host, port })
    }

    /// The one spelling policies are written against.
    pub(crate) fn canonical(&self) -> String {
        match self.port {
            Some(port) => format!("{}://{}:{port}", self.scheme, self.host),
            None => format!("{}://{}", self.scheme, self.host),
        }
    }

    /// The port the origin is on, written out even when it is the default.
    ///
    /// Exists so a pattern such as `http://localhost:*` matches
    /// `http://localhost`, whose canonical form has no port for the wildcard to
    /// match against.
    pub(crate) fn explicit_port(&self) -> u16 {
        self.port.unwrap_or_else(|| default_port(self.scheme))
    }
}

/// An origin pattern from a policy, canonicalised but not yet compiled.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct OriginPattern {
    /// The canonical pattern, as an operator would read it in an audit record.
    pub(crate) source: String,
    /// What the pattern matches, part by part.
    pub(crate) parts: PatternParts,
}

/// The globs an origin pattern is compiled from.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PatternParts {
    /// A pattern of nothing but wildcards: any origin at all.
    Any,
    /// A scheme, host and port, each matched on its own.
    Parts {
        /// The scheme glob, lower-case.
        scheme: String,
        /// The host glob. Differs from how the host reads only for an IPv6
        /// literal, whose brackets would otherwise be read as a character class.
        host: String,
        /// The port glob, or `None` for the scheme's default port.
        port: Option<String>,
    },
}

/// A compiled origin pattern.
///
/// Scheme, host and port are matched separately so that a host glob can never
/// absorb a port: `https://*` admits every host on port 443 and no other port.
///
/// `None` inside is the any-origin pattern, which has no parts to match.
#[derive(Debug, Clone)]
pub struct OriginMatcher(Option<Box<PartMatchers>>);

#[derive(Debug, Clone)]
struct PartMatchers {
    scheme: GlobMatcher,
    host: GlobMatcher,
    port: PortMatcher,
}

#[derive(Debug, Clone)]
enum PortMatcher {
    /// No port was written: the request must be on its scheme's default.
    Default,
    /// Matched against the request's port, written out even when default, so
    /// `:*` includes the default port.
    Glob(GlobMatcher),
}

impl OriginMatcher {
    /// Compile a canonicalised pattern.
    fn compile(parts: &PatternParts) -> Result<Self, globset::Error> {
        let compile = |glob: &str| Glob::new(glob).map(|glob| glob.compile_matcher());
        Ok(Self(match parts {
            PatternParts::Any => None,
            PatternParts::Parts { scheme, host, port } => Some(Box::new(PartMatchers {
                scheme: compile(scheme)?,
                host: compile(host)?,
                port: match port {
                    None => PortMatcher::Default,
                    Some(port) => PortMatcher::Glob(compile(port)?),
                },
            })),
        }))
    }

    /// Whether this pattern admits `origin`.
    ///
    /// The origin is canonicalised here as well as by the tool that produced
    /// it, so the guarantee does not depend on every tool remembering to. An
    /// origin that cannot be parsed is matched only by the any-origin pattern:
    /// tools refuse such URLs before planning, so none reach here from them.
    #[must_use]
    pub fn is_match(&self, origin: &str) -> bool {
        let Some(parts) = &self.0 else {
            return true;
        };
        let Ok(parsed) = Origin::parse(origin) else {
            return false;
        };
        parts.scheme.is_match(parsed.scheme)
            && parts.host.is_match(&parsed.host)
            && match &parts.port {
                PortMatcher::Default => parsed.port.is_none(),
                PortMatcher::Glob(glob) => glob.is_match(parsed.explicit_port().to_string()),
            }
    }
}

/// Canonicalise and compile an origin pattern from a policy.
///
/// Returns the canonical source, for audit records, and its matcher.
///
/// # Errors
///
/// [`PatternError::Origin`] when the pattern is not `scheme://host[:port]`, and
/// [`PatternError::Glob`] when one of its parts is not a valid glob.
pub(crate) fn compile_origin_pattern(raw: &str) -> Result<(String, OriginMatcher), PatternError> {
    let pattern =
        canonicalise_origin_pattern(raw).map_err(|reason| PatternError::Origin { reason })?;
    let matcher = OriginMatcher::compile(&pattern.parts)?;
    Ok((pattern.source, matcher))
}

/// Canonicalise an origin pattern the way [`normalise_origin`] canonicalises a
/// URL, leaving glob syntax in place.
///
/// The error is a sentence saying what was wrong; the caller adds the expected
/// form.
pub(crate) fn canonicalise_origin_pattern(raw: &str) -> Result<OriginPattern, String> {
    // A pattern that is nothing but wildcards means "any origin" and has no
    // parts to canonicalise.
    if !raw.is_empty() && raw.chars().all(|character| character == '*') {
        return Ok(OriginPattern {
            source: raw.to_owned(),
            parts: PatternParts::Any,
        });
    }

    let (scheme, rest) = raw
        .split_once("://")
        .ok_or_else(|| "it has no scheme".to_owned())?;
    let scheme = scheme.to_ascii_lowercase();
    let literal_scheme = if is_glob(&scheme) {
        if !scheme
            .chars()
            .all(|character| character.is_ascii_alphabetic() || is_glob_syntax(character))
        {
            return Err(format!("`{scheme}` is not a scheme"));
        }
        None
    } else {
        Some(web_scheme(&scheme).ok_or_else(|| {
            format!("`{scheme}` is not a browsable scheme; only http and https are")
        })?)
    };

    // One trailing slash is tolerated as a way of writing no path at all.
    // Anything longer is a path, which an origin never has, so the rule could
    // never match.
    let authority = match rest.split_once('/') {
        None => rest,
        Some((authority, "")) => authority,
        Some((_, path)) => {
            return Err(format!("it has a path (`/{path}`), which origins never do"));
        }
    };
    if authority.is_empty() {
        return Err("it has no host".to_owned());
    }
    if authority.contains('@') {
        return Err("it carries credentials, which a request is never allowed to".to_owned());
    }

    let (host, host_glob, port) = match ipv6_pattern_host(authority) {
        Some(parsed) => parsed?,
        None => {
            let (host, port) = authority
                .split_once(':')
                .map_or((authority, None), |(host, port)| (host, Some(port)));
            if host.is_empty() {
                return Err("it has no host".to_owned());
            }
            let host = pattern_host(host)?;
            (host.clone(), host, port)
        }
    };

    let port = match port {
        None => None,
        Some("") => return Err("it has an empty port".to_owned()),
        Some(port) if is_glob(port) => {
            if !port
                .chars()
                .all(|character| character.is_ascii_digit() || is_glob_syntax(character))
            {
                return Err(format!("`{port}` is not a port"));
            }
            Some(port.to_owned())
        }
        Some(port) => {
            let number = parse_port(port)
                .ok_or_else(|| format!("`{port}` is not a port from 1 to 65535"))?;
            // Writing the default port is the same as writing none. Under a
            // scheme glob there is no one default, so the number is kept and
            // matched against the request's port written out.
            let droppable = literal_scheme.is_some_and(|scheme| number == default_port(scheme));
            (!droppable).then(|| number.to_string())
        }
    };

    let written_port = port
        .as_deref()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Ok(OriginPattern {
        source: format!("{scheme}://{host}{written_port}"),
        parts: PatternParts::Parts {
            scheme,
            host: host_glob,
            port,
        },
    })
}

/// A pattern's host as read, its host as globbed, and its port, if any.
type PatternAuthority<'a> = (String, String, Option<&'a str>);

/// A bracketed IPv6 host at the start of a pattern's authority, if there is one.
///
/// Returns `None` when the authority does not start with an IPv6 literal, so
/// that a leading character class such as `[ab].example.com` is still read as
/// a glob. The literal's brackets are escaped in the glob as one-character
/// classes, which globset reads the same on every platform.
fn ipv6_pattern_host(authority: &str) -> Option<Result<PatternAuthority<'_>, String>> {
    let inner = authority.strip_prefix('[')?;
    let (address, after) = inner.split_once(']')?;
    let address: Ipv6Addr = address.parse().ok()?;
    let Some(address) = ipv6_literal(address) else {
        return Some(Err(format!(
            "`[{address}]` is an IPv4 address or the unspecified address written as IPv6, \
             which a request is never allowed to be"
        )));
    };
    let port = match after {
        "" => None,
        after => match after.strip_prefix(':') {
            Some(port) => Some(port),
            None => {
                return Some(Err(format!(
                    "`{after}` follows the IPv6 address where only a port may"
                )));
            }
        },
    };
    let glob = format!("[[]{}[]]", &address[1..address.len() - 1]);
    Some(Ok((address, glob, port)))
}

/// Canonicalise the host half of a pattern. Literal hosts get the full
/// treatment; globbed hosts are lower-cased and checked for stray characters.
fn pattern_host(host: &str) -> Result<String, String> {
    if !is_glob(host) {
        return literal_host(host).ok_or_else(|| format!("`{host}` is not a valid host"));
    }
    let mut lowered = host.to_ascii_lowercase();
    if let Some(stray) = lowered
        .chars()
        .find(|character| !is_host_character(*character) && !is_glob_syntax(*character))
    {
        return Err(format!("`{stray}` cannot appear in a host"));
    }
    if lowered.ends_with('.') {
        lowered.pop();
    }
    Ok(lowered)
}

/// `http` or `https` in any case, as a static string.
fn web_scheme(scheme: &str) -> Option<&'static str> {
    if scheme.eq_ignore_ascii_case("http") {
        Some("http")
    } else if scheme.eq_ignore_ascii_case("https") {
        Some("https")
    } else {
        None
    }
}

/// The port a scheme implies when none is written.
fn default_port(scheme: &str) -> u16 {
    if scheme == "https" { 443 } else { 80 }
}

/// Split an authority into host and optional port, keeping IPv6 brackets on the
/// host. `None` for a bracketed host followed by anything but a port.
fn split_authority(authority: &str) -> Option<(&str, Option<&str>)> {
    if authority.starts_with('[') {
        let end = authority.find(']')?;
        let (host, after) = authority.split_at(end + 1);
        return match after {
            "" => Some((host, None)),
            after => after.strip_prefix(':').map(|port| (host, Some(port))),
        };
    }
    Some(
        authority
            .split_once(':')
            .map_or((authority, None), |(host, port)| (host, Some(port))),
    )
}

/// A port of 1-65535 written in ASCII digits.
///
/// Checked character by character because `u16::from_str` also accepts a
/// leading `+`. Leading zeros are accepted and dropped, as a browser does.
fn parse_port(raw: &str) -> Option<u16> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    raw.parse::<u16>().ok().filter(|port| *port != 0)
}

/// Canonicalise a literal host, or refuse it.
///
/// A bracketed IPv6 literal is parsed and re-rendered in compressed form. A
/// name is lower-cased and loses one trailing dot, and must be dot-separated
/// labels of letters, digits, `-` and `_`. A name whose last label is numeric
/// is one a browser would parse as an IPv4 address; it is accepted only in the
/// dotted-decimal form it would be rewritten to, so `http://2130706433` cannot
/// reach 127.0.0.1 under a rule that names neither. For the same reason an IPv4
/// address written as IPv6 is refused, and so is the `0.0.0.0/8` block, whose
/// unspecified address reaches local listeners.
fn literal_host(host: &str) -> Option<String> {
    if let Some(inner) = host.strip_prefix('[') {
        let address: Ipv6Addr = inner.strip_suffix(']')?.parse().ok()?;
        return ipv6_literal(address);
    }
    let lowered = host.to_ascii_lowercase();
    let name = lowered.strip_suffix('.').unwrap_or(&lowered);
    if name.is_empty() || name.split('.').any(str::is_empty) || !name.chars().all(is_host_character)
    {
        return None;
    }
    let last = name.rsplit('.').next().unwrap_or_default();
    let numeric = last.bytes().all(|byte| byte.is_ascii_digit())
        || last
            .strip_prefix("0x")
            .is_some_and(|hex| hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    if numeric {
        let address: Ipv4Addr = name.parse().ok()?;
        // `0.0.0.0` connects to this host, and the rest of `0.0.0.0/8` is
        // "this network": neither is a server a policy could mean to name.
        if address.octets()[0] == 0 {
            return None;
        }
        return Some(address.to_string());
    }
    Some(name.to_owned())
}

/// A bracketed IPv6 literal in compressed form, unless it is another spelling
/// of an IPv4 address or the unspecified address.
///
/// `[::ffff:127.0.0.1]` — also written `[::ffff:7f00:1]` — reaches 127.0.0.1
/// over a dual-stack socket, and the deprecated compatible form `[::7f00:1]`
/// embeds it the same way. Accepting either would let a request reach loopback
/// under a rule that names neither `127.0.0.1` nor `[::1]`. `[::]`, like
/// `0.0.0.0`, reaches local listeners. Loopback `[::1]` is its own address and
/// stays.
fn ipv6_literal(address: Ipv6Addr) -> Option<String> {
    // `to_ipv4` is `Some` for the mapped and compatible forms and for `[::]`;
    // it also maps `[::1]` to 0.0.0.1, which is why loopback is let through.
    if address.to_ipv4().is_some() && !address.is_loopback() {
        return None;
    }
    Some(format!("[{address}]"))
}

/// Characters a literal host name may contain.
const fn is_host_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_')
}

/// Characters with meaning in a globset pattern.
const fn is_glob_syntax(character: char) -> bool {
    matches!(
        character,
        '*' | '?' | '[' | ']' | '{' | '}' | ',' | '!' | '^' | '-'
    )
}

/// Whether a pattern component contains a wildcard, class or alternation.
fn is_glob(component: &str) -> bool {
    component.contains(['*', '?', '[', '{'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalised(url: &str) -> String {
        normalise_origin(url).unwrap()
    }

    fn pattern(raw: &str) -> String {
        canonicalise_origin_pattern(raw).unwrap().source
    }

    #[test]
    fn case_and_default_port_reduce_to_one_spelling() {
        // The bypass this module exists for: two spellings of one server were
        // two policy subjects.
        assert_eq!(
            normalised("https://API.Example.com"),
            "https://api.example.com"
        );
        assert_eq!(
            normalised("https://api.example.com:443"),
            "https://api.example.com"
        );
        assert_eq!(
            normalised("HTTPS://api.example.com:443/"),
            "https://api.example.com"
        );
    }

    #[test]
    fn path_query_and_fragment_are_dropped() {
        assert_eq!(normalised("http://host:80/a?b#c"), "http://host");
        assert_eq!(normalised("http://host?q"), "http://host");
        assert_eq!(normalised("http://host#f"), "http://host");
    }

    #[test]
    fn a_non_default_port_is_kept() {
        assert_eq!(normalised("https://host:8443"), "https://host:8443");
        assert_eq!(normalised("http://host:443"), "http://host:443");
        assert_eq!(normalised("https://host:80"), "https://host:80");
        // Leading zeros are the same number, and a browser drops them too.
        assert_eq!(normalised("https://host:0443"), "https://host");
    }

    #[test]
    fn credentials_are_refused() {
        assert!(matches!(
            normalise_origin("https://user:pw@host"),
            Err(OriginError::CredentialsInUrl { .. })
        ));
        // Reads as trusted.example to a human and goes to evil.example.
        assert!(matches!(
            normalise_origin("https://trusted.example@evil.example"),
            Err(OriginError::CredentialsInUrl { .. })
        ));
    }

    #[test]
    fn only_web_schemes_are_origins() {
        for url in [
            "file:///etc/passwd",
            "data:text/html,<h1>x",
            "javascript:alert(1)",
            "ftp://example.com",
            "host/path",
            "not a url",
        ] {
            assert!(
                matches!(
                    normalise_origin(url),
                    Err(OriginError::UnsupportedScheme { .. })
                ),
                "`{url}` should be refused as not http(s)"
            );
        }
        assert!(matches!(
            normalise_origin("https://"),
            Err(OriginError::NoHost { .. })
        ));
    }

    #[test]
    fn ports_outside_the_range_are_refused() {
        for url in [
            "https://host:0",
            "https://host:65536",
            "https://host:+80",
            "https://host:",
            "https://host:8o",
            "https://:8080",
        ] {
            assert!(
                matches!(normalise_origin(url), Err(OriginError::InvalidPort { .. })),
                "`{url}` should be refused for its port"
            );
        }
    }

    #[test]
    fn a_backslash_ends_the_host_as_it_does_in_a_browser() {
        // Read as one string, this is a subdomain of trusted.example and a glob
        // `https://*.trusted.example` would admit it. The browser goes to
        // evil.example.
        assert_eq!(
            normalised(r"https://evil.example\x.trusted.example/"),
            "https://evil.example"
        );
    }

    #[test]
    fn hosts_a_browser_would_rewrite_are_refused() {
        for url in [
            "https://%65vil.example",
            "https://bücher.example",
            "https://exa mple.com",
            "https://a..example",
            "http://2130706433",
            "http://0x7f.0.0.1",
            "http://127.1",
            "http://0177.0.0.1",
            "http://[::1",
            "http://[::1]x",
            "http://[fe80::1%25en0]",
            // IPv4 written as IPv6 reaches the IPv4 host over a dual-stack
            // socket, so a deny naming 127.0.0.1 would not bind it.
            "http://[::ffff:127.0.0.1]",
            "http://[::ffff:7f00:1]:8420",
            "http://[::7f00:1]",
            // The unspecified addresses reach local listeners.
            "http://0.0.0.0:8420",
            "http://0.1.2.3",
            "http://[::]",
            "http://[0::0]:8080",
        ] {
            assert!(
                matches!(normalise_origin(url), Err(OriginError::InvalidHost { .. })),
                "`{url}` should be refused for its host"
            );
        }
    }

    #[test]
    fn address_literals_have_one_spelling() {
        assert_eq!(
            normalised("http://127.0.0.1:8420/"),
            "http://127.0.0.1:8420"
        );
        assert_eq!(normalised("http://[0:0::1]:8080/"), "http://[::1]:8080");
        assert_eq!(normalised("http://[::1]:80/"), "http://[::1]");
        assert_eq!(normalised("https://Example.com./"), "https://example.com");
    }

    #[test]
    fn explicit_port_names_the_default() {
        let origin = Origin::parse("http://localhost/").unwrap();
        assert_eq!(origin.canonical(), "http://localhost");
        assert_eq!(origin.explicit_port(), 80);
        let origin = Origin::parse("https://localhost:8443/").unwrap();
        assert_eq!(origin.explicit_port(), 8443);
    }

    #[test]
    fn loopback_and_ordinary_ipv6_literals_are_kept() {
        assert_eq!(normalised("http://[::1]:8420"), "http://[::1]:8420");
        assert_eq!(normalised("http://[2001:DB8::1]"), "http://[2001:db8::1]");
        assert_eq!(normalised("http://10.0.0.1"), "http://10.0.0.1");
    }

    #[test]
    fn patterns_canonicalise_like_requests() {
        assert_eq!(
            pattern("https://API.Example.com"),
            "https://api.example.com"
        );
        assert_eq!(
            pattern("https://api.example.com:443"),
            "https://api.example.com"
        );
        assert_eq!(pattern("HTTP://localhost:80/"), "http://localhost");
        assert_eq!(pattern("http://[0::1]:8080"), "http://[::1]:8080");
        assert_eq!(pattern("http://[::1]:80"), "http://[::1]");
    }

    #[test]
    fn patterns_keep_their_glob_syntax() {
        assert_eq!(pattern("https://*.Example.com"), "https://*.example.com");
        assert_eq!(pattern("http://localhost:*"), "http://localhost:*");
        assert_eq!(pattern("http://127.0.0.1:*"), "http://127.0.0.1:*");
        assert_eq!(pattern("*"), "*");
        assert_eq!(pattern("*://Example.com"), "*://example.com");
        assert_eq!(
            pattern("https://{a,B}.example.com"),
            "https://{a,b}.example.com"
        );
        // A host glob is matched apart from the port, so the default port is
        // the same as none on a glob host too.
        assert_eq!(pattern("https://*:443"), "https://*");
        // Under a scheme glob there is no single default to drop.
        assert_eq!(pattern("*://example.com:443"), "*://example.com:443");
    }

    #[test]
    fn ipv6_literal_patterns_escape_their_brackets() {
        let parsed = canonicalise_origin_pattern("http://[::1]:*").unwrap();
        assert_eq!(parsed.source, "http://[::1]:*");
        assert_eq!(
            parsed.parts,
            PatternParts::Parts {
                scheme: "http".to_owned(),
                host: "[[]::1[]]".to_owned(),
                port: Some("*".to_owned()),
            }
        );
    }

    #[test]
    fn patterns_that_name_a_respelt_or_unspecified_address_are_refused() {
        for raw in [
            "http://[::ffff:127.0.0.1]:*",
            "http://[::ffff:7f00:1]",
            "http://[::]",
            "http://0.0.0.0:*",
        ] {
            assert!(
                canonicalise_origin_pattern(raw).is_err(),
                "`{raw}` should be refused"
            );
        }
    }

    #[test]
    fn malformed_patterns_say_what_is_wrong() {
        for (raw, complaint) in [
            ("example.com", "no scheme"),
            ("ftp://example.com", "not a browsable scheme"),
            ("https://example.com/app", "has a path"),
            ("https://user@example.com", "credentials"),
            ("https://example.com:99999", "not a port"),
            ("https://example.com:", "empty port"),
            ("https://", "no host"),
            ("https://:8080", "no host"),
            ("https://ex ample.com", "not a valid host"),
            ("https://*.ex%61mple.com", "cannot appear in a host"),
        ] {
            let error = canonicalise_origin_pattern(raw).unwrap_err();
            assert!(
                error.contains(complaint),
                "`{raw}` produced `{error}`, expected it to mention `{complaint}`"
            );
        }
    }
}
