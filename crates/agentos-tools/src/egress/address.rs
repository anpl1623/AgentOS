//! Which addresses a request may connect to.
//!
//! An address is first reduced to the IPv4 address it embeds, if it embeds
//! one, then judged against fixed tables. The [`AddressPolicy`] an [`Egress`]
//! was built with decides only whether the private ranges, and for tests
//! loopback, are admitted; nothing else in the tables can be relaxed.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::Egress;

/// Which addresses a request may connect to, beyond public unicast.
///
/// Fixed when an [`Egress`] is built. For `network.request` it is always
/// [`Self::Strict`]; for an integration account it is decided by a flag only
/// the operator can set, and no tool argument reaches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AddressPolicy {
    /// Public unicast addresses only.
    #[default]
    Strict,
    /// Public addresses and the private ranges an operator's own server can
    /// live in: RFC 1918, IPv6 unique-local and carrier-grade NAT.
    ///
    /// For a self-hosted service such as GitHub Enterprise on the operator's
    /// network. Loopback, unspecified, link-local, multicast, broadcast and
    /// documentation addresses are refused as they are under [`Self::Strict`]:
    /// none of them is where an operator's server lives, and link-local is
    /// where most clouds' metadata services do. The ones that answer inside the
    /// admitted ranges, Alibaba Cloud's `100.100.100.200` and AWS's
    /// `fd00:ec2::/32`, are refused by name.
    AllowPrivateNetwork,
}

impl Egress {
    /// Why `address` may not be connected to, or `None` when it may.
    ///
    /// The address is first reduced to the IPv4 address it carries, if it
    /// carries one, so that it is judged by where it actually reaches.
    #[must_use]
    pub fn refusal(&self, address: IpAddr) -> Option<&'static str> {
        let reason = match canonical(address) {
            IpAddr::V4(v4) => ipv4_refusal(v4),
            IpAddr::V6(v6) => ipv6_refusal(v6),
        }?;
        let admitted = match reason {
            LOOPBACK => self.loopback,
            PRIVATE | UNIQUE_LOCAL | SHARED => self.addresses == AddressPolicy::AllowPrivateNetwork,
            _ => false,
        };
        (!admitted).then_some(reason)
    }
}

/// The host as an address, when the URL wrote one.
pub(super) fn ip_literal(host: &str) -> Option<IpAddr> {
    let unbracketed = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    unbracketed.parse().ok()
}

/// The IPv4 address an IPv6 address stands for, if it stands for one.
///
/// IPv4-mapped (`::ffff:a.b.c.d`) and IPv4-compatible (`::a.b.c.d`) addresses
/// are the IPv4 address. IPv4-translated (`::ffff:0:a.b.c.d`), NAT64
/// (`64:ff9b::/96`) and 6to4 (`2002::/16`) addresses reach the IPv4 address
/// they embed through a translator or a relay. The unspecified and loopback
/// addresses are left as IPv6, since `::` and `::1` would otherwise read as
/// `0.0.0.0` and `0.0.0.1`.
#[must_use]
pub fn canonical(address: IpAddr) -> IpAddr {
    let IpAddr::V6(v6) = address else {
        return address;
    };
    if v6.is_unspecified() || v6.is_loopback() {
        return address;
    }
    if let Some(v4) = v6.to_ipv4_mapped() {
        return IpAddr::V4(v4);
    }
    let segments = v6.segments();
    let octets = v6.octets();
    let low = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
    if segments[..6].iter().all(|segment| *segment == 0) {
        return IpAddr::V4(low);
    }
    if segments[..4].iter().all(|segment| *segment == 0)
        && segments[4] == 0xffff
        && segments[5] == 0
    {
        return IpAddr::V4(low);
    }
    if segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2..6].iter().all(|s| *s == 0) {
        return IpAddr::V4(low);
    }
    if segments[0] == 0x2002 {
        return IpAddr::V4(Ipv4Addr::new(octets[2], octets[3], octets[4], octets[5]));
    }
    address
}

const LOOPBACK: &str = "a loopback address";
const PRIVATE: &str = "a private address";
const UNIQUE_LOCAL: &str = "a unique-local address";
const SHARED: &str = "a shared (carrier-grade NAT) address";

/// Named apart from the range it sits in, because it sits in one the private
/// network policy admits: Alibaba Cloud's metadata service answers at
/// `100.100.100.200`, inside carrier-grade NAT, and AWS's IPv6 one at
/// `fd00:ec2::254`, inside unique-local. A refusal that relied on metadata
/// living in link-local would admit both under
/// [`AddressPolicy::AllowPrivateNetwork`].
const CLOUD_METADATA: &str = "a cloud metadata address";

fn ipv4_refusal(address: Ipv4Addr) -> Option<&'static str> {
    let [a, b, c, _] = address.octets();
    if address == Ipv4Addr::new(100, 100, 100, 200) {
        return Some(CLOUD_METADATA);
    }
    Some(match (a, b, c) {
        (0, _, _) => "an unspecified address",
        (127, _, _) => LOOPBACK,
        (10, _, _) | (192, 168, _) => PRIVATE,
        (172, 16..=31, _) => PRIVATE,
        (100, 64..=127, _) => SHARED,
        (169, 254, _) => "a link-local address",
        (192, 0, 0) => "reserved for protocol assignments",
        (192, 0, 2) | (198, 51, 100) | (203, 0, 113) => "a documentation address",
        (192, 88, 99) => "a deprecated relay address",
        (198, 18..=19, _) => "a benchmarking address",
        (224..=239, _, _) => "a multicast address",
        _ if address.is_broadcast() => "the broadcast address",
        (240..=255, _, _) => "a reserved address",
        _ => return None,
    })
}

fn ipv6_refusal(address: Ipv6Addr) -> Option<&'static str> {
    let segments = address.segments();
    Some(if address.is_unspecified() {
        "an unspecified address"
    } else if address.is_loopback() {
        LOOPBACK
    } else if segments[0] == 0xfd00 && segments[1] == 0x0ec2 {
        // The whole of AWS's `fd00:ec2::/32`: the instance metadata service,
        // its DNS resolver and its time server all answer inside it.
        CLOUD_METADATA
    } else if segments[0] & 0xfe00 == 0xfc00 {
        UNIQUE_LOCAL
    } else if segments[0] & 0xffc0 == 0xfe80 {
        "a link-local address"
    } else if segments[0] & 0xffc0 == 0xfec0 {
        "a site-local address"
    } else if segments[0] & 0xff00 == 0xff00 {
        "a multicast address"
    } else if (segments[0] == 0x2001 && segments[1] == 0x0db8) || segments[0] & 0xfff0 == 0x3ff0 {
        "a documentation address"
    } else if segments[0] == 0x2001 && segments[1] < 0x0200 {
        // Teredo, benchmarking, ORCHID and the rest of the IETF's block. A
        // Teredo address hides the IPv4 address it reaches, so it cannot be
        // judged by it.
        "reserved for protocol assignments"
    } else if segments[0] == 0x0100 && segments[1..4].iter().all(|s| *s == 0) {
        "a discard-only address"
    } else if segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2] == 0x0001 {
        "a local-use NAT64 address"
    } else if segments[0] & 0xe000 != 0x2000 {
        // The ranges above are named so that a refusal says what was reached.
        // This one is the rule: everything outside 2000::/3 is unassigned or
        // assigned to something other than the public internet, and a range
        // nobody has named yet is refused rather than reached.
        "outside the global unicast range"
    } else {
        return None;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(address: &str) -> IpAddr {
        address.parse().unwrap()
    }

    /// Addresses no policy admits, written as themselves and as the IPv6 forms
    /// that reach them.
    const NEVER: &[&str] = &[
        "0.0.0.0",
        "169.254.169.254",
        "169.254.0.1",
        "192.0.2.1",
        "198.51.100.7",
        "203.0.113.9",
        "224.0.0.1",
        "255.255.255.255",
        "240.0.0.1",
        "192.0.0.8",
        "198.18.0.1",
        "::",
        "fe80::1",
        "ff02::1",
        "fec0::1",
        "2001:db8::1",
        "::ffff:169.254.169.254",
        "64:ff9b::a9fe:a9fe",
        "2002:a9fe:a9fe::1",
        "::ffff:0:a9fe:a9fe",
        // Cloud metadata outside link-local, inside ranges the private
        // network policy admits.
        "100.100.100.200",
        "::ffff:100.100.100.200",
        "64:ff9b::6464:64c8",
        "fd00:ec2::254",
        "fd00:ec2::253",
        "fd00:ec2::123",
        "fd00:ec2::23",
    ];

    /// What an operator's own server can live at.
    const PRIVATE_NETWORK: &[&str] = &[
        "10.0.0.1",
        "10.255.255.254",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "100.64.0.1",
        "100.127.255.254",
        "fc00::1",
        "fd12:3456::1",
        "::ffff:10.0.0.1",
        "::ffff:192.168.1.1",
    ];

    const LOOPBACK_FORMS: &[&str] = &["127.0.0.1", "127.8.9.10", "::1", "::ffff:127.0.0.1"];

    #[test]
    fn strict_admits_public_unicast_only() {
        let strict = Egress::new(AddressPolicy::Strict);
        for address in NEVER.iter().chain(PRIVATE_NETWORK).chain(LOOPBACK_FORMS) {
            assert!(strict.refusal(ip(address)).is_some(), "{address}");
        }
        for address in ["93.184.216.34", "140.82.112.6", "2606:4700::1111"] {
            assert_eq!(strict.refusal(ip(address)), None, "{address}");
        }
    }

    #[test]
    fn the_private_network_policy_admits_the_operators_ranges_and_nothing_else() {
        let private = Egress::new(AddressPolicy::AllowPrivateNetwork);
        for address in PRIVATE_NETWORK {
            assert_eq!(private.refusal(ip(address)), None, "{address}");
        }
        // Not loopback, so a bound account cannot be pointed at a service on
        // the operator's own machine; and not link-local, where the cloud's
        // metadata service answers.
        for address in NEVER.iter().chain(LOOPBACK_FORMS) {
            assert!(private.refusal(ip(address)).is_some(), "{address}");
        }
        // Nor the metadata services that answer inside the ranges it admits,
        // and the refusal says which they are.
        for address in ["100.100.100.200", "::ffff:100.100.100.200", "fd00:ec2::254"] {
            assert_eq!(
                private.refusal(ip(address)),
                Some(CLOUD_METADATA),
                "{address}"
            );
        }
        assert_eq!(private.refusal(ip("93.184.216.34")), None);
    }

    #[test]
    fn loopback_is_admitted_only_by_the_test_constructor() {
        for policy in [AddressPolicy::Strict, AddressPolicy::AllowPrivateNetwork] {
            assert!(!Egress::new(policy).admits_loopback());
            let test = Egress::for_tests_admitting_loopback(policy);
            assert!(test.admits_loopback());
            for address in LOOPBACK_FORMS {
                assert_eq!(test.refusal(ip(address)), None, "{address}");
            }
            for address in NEVER {
                assert!(test.refusal(ip(address)).is_some(), "{address}");
            }
        }
        // Loopback for tests does not bring the private network with it.
        let test = Egress::for_tests_admitting_loopback(AddressPolicy::Strict);
        assert!(test.refusal(ip("10.0.0.1")).is_some());
    }

    #[test]
    fn the_default_transport_is_strict() {
        assert_eq!(AddressPolicy::default(), AddressPolicy::Strict);
        assert_eq!(Egress::default(), Egress::new(AddressPolicy::Strict));
        assert_eq!(Egress::default().address_policy(), AddressPolicy::Strict);
    }
}
