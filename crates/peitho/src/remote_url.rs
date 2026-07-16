use std::{collections::HashSet, net::IpAddr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteUrlCandidate {
    pub address: IpAddr,
    pub label: Option<RemoteUrlLabel>,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteUrlLabel {
    Tailscale,
}

impl RemoteUrlLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tailscale => "Tailscale",
        }
    }
}

pub fn remote_url_candidates(
    addrs: &[IpAddr],
    default_route: Option<IpAddr>,
    port: u16,
    bound_wildcard: Option<IpAddr>,
) -> Vec<RemoteUrlCandidate> {
    let mut seen = HashSet::new();
    let mut candidates = addrs
        .iter()
        .copied()
        .filter(|addr| !is_excluded_addr(*addr))
        .filter(|addr| matches_bound_wildcard_family(*addr, bound_wildcard))
        .filter(|addr| seen.insert(*addr))
        .map(|address| RemoteUrlCandidate {
            address,
            label: remote_url_label(address),
            url: remote_url_for_addr(address, port),
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| candidate_rank(candidate.address, default_route));
    candidates
}

fn matches_bound_wildcard_family(address: IpAddr, bound_wildcard: Option<IpAddr>) -> bool {
    match bound_wildcard {
        Some(IpAddr::V4(wildcard)) if wildcard.is_unspecified() => address.is_ipv4(),
        Some(IpAddr::V6(wildcard)) if wildcard.is_unspecified() => {
            // macOS and stock Linux default to dual-stack IPv6 wildcard listeners
            // (IPV6_V6ONLY off), so IPv4-mapped clients can still connect.
            true
        }
        _ => true,
    }
}

fn candidate_rank(address: IpAddr, default_route: Option<IpAddr>) -> u8 {
    if Some(address) == default_route {
        0
    } else if is_tailscale_addr(address) {
        1
    } else {
        2
    }
}

fn remote_url_label(address: IpAddr) -> Option<RemoteUrlLabel> {
    is_tailscale_addr(address).then_some(RemoteUrlLabel::Tailscale)
}

pub fn remote_url_for_addr(address: IpAddr, port: u16) -> String {
    match address {
        IpAddr::V4(address) => format!("http://{address}:{port}/remote"),
        IpAddr::V6(address) => format!("http://[{address}]:{port}/remote"),
    }
}

fn is_excluded_addr(address: IpAddr) -> bool {
    address.is_loopback() || is_link_local_addr(address)
}

fn is_link_local_addr(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            octets[0] == 169 && octets[1] == 254
        }
        IpAddr::V6(address) => {
            let octets = address.octets();
            octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80
        }
    }
}

fn is_tailscale_addr(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            octets[0] == 100 && (64..=127).contains(&octets[1])
        }
        IpAddr::V6(address) => {
            let segments = address.segments();
            segments[0] == 0xfd7a && segments[1] == 0x115c && segments[2] == 0xa1e0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn remote_url_candidates_exclude_loopback_and_link_local() {
        let candidates = remote_url_candidates(
            &[
                v4(127, 0, 0, 1),
                v4(169, 254, 10, 20),
                IpAddr::V6(Ipv6Addr::LOCALHOST),
                "fe80::1".parse().unwrap(),
                v4(192, 168, 1, 20),
            ],
            None,
            4321,
            None,
        );

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.url.as_str())
                .collect::<Vec<_>>(),
            vec!["http://192.168.1.20:4321/remote"]
        );
    }

    #[test]
    fn remote_url_candidates_label_cgnat_as_tailscale() {
        let candidates =
            remote_url_candidates(&[v4(100, 64, 0, 1), v4(100, 127, 255, 254)], None, 80, None);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].label, Some(RemoteUrlLabel::Tailscale));
        assert_eq!(candidates[1].label, Some(RemoteUrlLabel::Tailscale));
    }

    #[test]
    fn remote_url_candidates_label_tailscale_ipv6_ula() {
        let candidates =
            remote_url_candidates(&["fd7a:115c:a1e0::1".parse().unwrap()], None, 80, None);

        assert_eq!(candidates[0].label, Some(RemoteUrlLabel::Tailscale));
    }

    #[test]
    fn remote_url_candidates_order_default_route_then_tailscale_then_rest() {
        let candidates = remote_url_candidates(
            &[v4(192, 168, 1, 20), v4(100, 100, 10, 5), v4(10, 0, 0, 15)],
            Some(v4(10, 0, 0, 15)),
            3000,
            None,
        );

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "http://10.0.0.15:3000/remote",
                "http://100.100.10.5:3000/remote",
                "http://192.168.1.20:3000/remote"
            ]
        );
        assert_eq!(candidates[1].label, Some(RemoteUrlLabel::Tailscale));
    }

    #[test]
    fn remote_url_candidates_order_tailscale_ipv6_before_rest() {
        let candidates = remote_url_candidates(
            &[
                "2001:db8::5".parse().unwrap(),
                "fd7a:115c:a1e0::1".parse().unwrap(),
            ],
            None,
            3000,
            None,
        );

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "http://[fd7a:115c:a1e0::1]:3000/remote",
                "http://[2001:db8::5]:3000/remote"
            ]
        );
        assert_eq!(candidates[0].label, Some(RemoteUrlLabel::Tailscale));
    }

    #[test]
    fn remote_url_candidates_for_ipv4_wildcard_keep_only_ipv4_candidates() {
        let candidates = remote_url_candidates(
            &[v4(192, 168, 1, 20), "2001:db8::5".parse().unwrap()],
            None,
            3000,
            Some(v4(0, 0, 0, 0)),
        );

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.url.as_str())
                .collect::<Vec<_>>(),
            vec!["http://192.168.1.20:3000/remote"]
        );
    }

    #[test]
    fn remote_url_candidates_for_ipv6_wildcard_keep_both_address_families() {
        let candidates = remote_url_candidates(
            &[v4(192, 168, 1, 20), "2001:db8::5".parse().unwrap()],
            None,
            3000,
            Some("::".parse().unwrap()),
        );

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "http://192.168.1.20:3000/remote",
                "http://[2001:db8::5]:3000/remote"
            ]
        );
    }

    #[test]
    fn remote_url_candidates_deduplicates_addresses() {
        let candidates = remote_url_candidates(
            &[
                v4(192, 168, 1, 20),
                v4(192, 168, 1, 20),
                v4(100, 64, 0, 5),
                v4(100, 64, 0, 5),
            ],
            None,
            3000,
            None,
        );

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "http://100.64.0.5:3000/remote",
                "http://192.168.1.20:3000/remote"
            ]
        );
    }

    #[test]
    fn remote_url_candidates_bracket_ipv6_urls() {
        let candidates = remote_url_candidates(&["2001:db8::5".parse().unwrap()], None, 8080, None);

        assert_eq!(candidates[0].url, "http://[2001:db8::5]:8080/remote");
    }
}
