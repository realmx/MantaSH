//! Read-only presentation of Linux `ss` socket rows; no remote action is derived from these fields.
use crate::monitor::Port;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProtocolFilter {
    #[default]
    All,
    Tcp,
    Udp,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PortSort {
    #[default]
    Ascending,
    Descending,
    Protocol,
}

/// A socket's full sampled identity plus an occurrence number for identical rows.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PortKey {
    protocol: String,
    state: String,
    local: String,
    peer: String,
    process: String,
    occurrence: usize,
}

#[derive(Clone, Debug)]
pub struct PortRow<'a> {
    pub source: &'a Port,
    pub key: PortKey,
    pub address: &'a str,
    pub label: &'a str,
    pub number: Option<u16>,
}

/// Keep the complete endpoint for copying; parse only a clearly delimited port suffix.
pub fn endpoint(local: &str) -> (&str, &str, Option<u16>) {
    let Some((address, port)) = local.rsplit_once(':') else {
        return (local, "", None);
    };
    let bracketed = address.starts_with('[');
    if (bracketed && !address.ends_with(']'))
        || (!bracketed && address.contains(':'))
        || address.is_empty()
        || port.is_empty()
    {
        return (local, "", None);
    }
    // `*` and service names are displayed verbatim, never represented as numeric ports.
    let number = if port.bytes().all(|byte| byte.is_ascii_digit()) {
        port.parse::<u16>().ok()
    } else {
        None
    };
    (address, port, number)
}

/// Extract the process IDs emitted by `ss -p`, preserving first-seen order.
///
/// The raw field can contain multiple `pid=` entries for one socket. IDs are
/// only accepted when they are immediately followed by decimal digits.
pub fn process_pids(value: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    let mut remaining = value;
    while let Some(offset) = remaining.find("pid=") {
        let digits = &remaining[offset + 4..];
        let length = digits
            .bytes()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if length > 0 {
            if let Ok(pid) = digits[..length].parse::<u32>() {
                if !pids.contains(&pid) {
                    pids.push(pid);
                }
            }
        }
        remaining = &digits[length..];
    }
    pids
}

fn protocol_matches(port: &Port, filter: ProtocolFilter) -> bool {
    match filter {
        ProtocolFilter::All => true,
        ProtocolFilter::Tcp => port.protocol.to_ascii_lowercase().starts_with("tcp"),
        ProtocolFilter::Udp => port.protocol.to_ascii_lowercase().starts_with("udp"),
    }
}

/// Search full raw socket fields, including hidden peer/process text, without merging rows.
pub fn visible<'a>(
    ports: &'a [Port],
    query: &str,
    protocol: ProtocolFilter,
    sort: PortSort,
) -> Vec<PortRow<'a>> {
    let query = query.trim().to_lowercase();
    let mut occurrences = HashMap::new();
    let mut rows: Vec<_> = ports
        .iter()
        .enumerate()
        .filter_map(|(index, port)| {
            let fingerprint = (
                port.protocol.as_str(),
                port.state.as_str(),
                port.local.as_str(),
                port.peer.as_str(),
                port.process.as_str(),
            );
            let occurrence = occurrences.entry(fingerprint).or_insert(0usize);
            let key = PortKey {
                protocol: port.protocol.clone(),
                state: port.state.clone(),
                local: port.local.clone(),
                peer: port.peer.clone(),
                process: port.process.clone(),
                occurrence: *occurrence,
            };
            *occurrence += 1;
            if !protocol_matches(port, protocol)
                || (!query.is_empty()
                    && ![
                        &port.local,
                        &port.peer,
                        &port.protocol,
                        &port.state,
                        &port.process,
                    ]
                    .iter()
                    .any(|field| field.to_lowercase().contains(&query)))
            {
                return None;
            }
            let (address, label, number) = endpoint(&port.local);
            Some((
                index,
                PortRow {
                    source: port,
                    key,
                    address,
                    label,
                    number,
                },
            ))
        })
        .collect();
    rows.sort_by(|(left_index, left), (right_index, right)| {
        let numeric = left.number.is_none().cmp(&right.number.is_none());
        let port_order = match sort {
            PortSort::Ascending | PortSort::Protocol => left.number.cmp(&right.number),
            PortSort::Descending => right.number.cmp(&left.number),
        };
        let protocol_order = left
            .source
            .protocol
            .to_ascii_lowercase()
            .cmp(&right.source.protocol.to_ascii_lowercase());
        let order = match sort {
            PortSort::Protocol => numeric.then(protocol_order).then(port_order),
            _ => numeric.then(port_order).then(protocol_order),
        };
        order
            .then_with(|| left.source.local.cmp(&right.source.local))
            .then_with(|| left.source.peer.cmp(&right.source.peer))
            .then_with(|| left.source.process.cmp(&right.source.process))
            .then_with(|| left_index.cmp(right_index))
    });
    rows.into_iter().map(|(_, row)| row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(protocol: &str, state: &str, local: &str, peer: &str, process: &str) -> Port {
        Port {
            protocol: protocol.into(),
            state: state.into(),
            local: local.into(),
            peer: peer.into(),
            process: process.into(),
        }
    }

    #[test]
    fn endpoints_preserve_ipv4_ipv6_wildcard_and_non_numeric_text() {
        for (input, host, label, number) in [
            ("0.0.0.0:22", "0.0.0.0", "22", Some(22)),
            ("[::]:443", "[::]", "443", Some(443)),
            ("[::1]:5353", "[::1]", "5353", Some(5353)),
            ("*:0", "*", "0", Some(0)),
            ("*:ssh", "*", "ssh", None),
            ("*:65536", "*", "65536", None),
            ("*: *", "*", " *", None),
            ("::1", "::1", "", None),
            ("unknown", "unknown", "", None),
            ("[::1:22", "[::1:22", "", None),
        ] {
            assert_eq!(endpoint(input), (host, label, number));
        }
    }

    #[test]
    fn search_filter_sort_and_duplicates_preserve_raw_rows() {
        let ports = vec![
            row("udp6", "UNCONN", "[::]:5353", "[::]:*", ""),
            row(
                "tcp",
                "LISTEN",
                "0.0.0.0:22",
                "*:*",
                "users:((sshd,pid=1,fd=3),(helper,pid=9,fd=5))",
            ),
            row(
                "tcp",
                "LISTEN",
                "0.0.0.0:22",
                "*:*",
                "users:((sshd,pid=1,fd=3),(helper,pid=9,fd=5))",
            ),
            row("tcp6", "LISTEN", "[::]:22", "[::]:*", ""),
            row("udp", "UNCONN", "*:ssh", "*:*", "dns"),
            row("sctp", "LISTEN", "*:80", "*:*", "server"),
        ];
        let all = visible(&ports, "", ProtocolFilter::All, PortSort::Ascending);
        assert_eq!(all.len(), 6);
        assert_eq!(
            all.iter().map(|r| r.label).collect::<Vec<_>>(),
            ["22", "22", "22", "80", "5353", "ssh"]
        );
        assert_ne!(all[0].key, all[1].key);
        let filtered = visible(&ports, "sshd", ProtocolFilter::Tcp, PortSort::Ascending);
        assert_eq!(filtered[0].key, all[0].key);
        assert_eq!(filtered[1].key, all[1].key);
        assert_eq!(
            visible(&ports, "helper", ProtocolFilter::Tcp, PortSort::Ascending).len(),
            2
        );
        assert_eq!(
            visible(&ports, "[::]:*", ProtocolFilter::Udp, PortSort::Ascending).len(),
            1
        );
        assert_eq!(
            visible(&ports, "LISTEN", ProtocolFilter::Udp, PortSort::Ascending).len(),
            0
        );
        assert_eq!(
            visible(&ports, "5353", ProtocolFilter::Udp, PortSort::Ascending).len(),
            1
        );
        let tcp = visible(&ports, "", ProtocolFilter::Tcp, PortSort::Descending);
        let descending = visible(&ports, "", ProtocolFilter::All, PortSort::Descending);
        assert_eq!(
            descending.iter().map(|r| r.number).collect::<Vec<_>>(),
            [Some(5353), Some(80), Some(22), Some(22), Some(22), None]
        );
        assert_eq!(
            visible(&ports, "SSHD", ProtocolFilter::All, PortSort::Ascending).len(),
            2
        );
        assert_eq!(
            visible(&ports, "", ProtocolFilter::Udp, PortSort::Ascending).len(),
            2
        );
        assert_eq!(
            visible(&ports, "", ProtocolFilter::All, PortSort::Ascending)[0].key,
            all[0].key
        );
        assert_eq!(tcp.len(), 3);
        assert!(tcp.iter().all(|r| r.number == Some(22)));
        let protocol = visible(&ports, "", ProtocolFilter::All, PortSort::Protocol);
        assert_eq!(protocol[0].source.protocol, "sctp");
        assert_eq!(protocol[1].source.protocol, "tcp");
        assert_eq!(protocol[4].source.protocol, "udp6");
        assert_eq!(protocol[5].source.protocol, "udp");
        assert_eq!(all[5].source.local, "*:ssh");
    }
    #[test]
    fn process_pids_extracts_distinct_ids_in_raw_order() {
        assert_eq!(
            process_pids("users:((sshd,pid=40,fd=4),(helper,pid=81,fd=7))"),
            vec![40, 81]
        );
        assert_eq!(
            process_pids("users:((sshd,pid=40,fd=4),(sshd,pid=40,fd=5))"),
            vec![40]
        );
        assert_eq!(
            process_pids("users:((broken,pid=x),(valid,pid=7,fd=3))"),
            vec![7]
        );
        assert!(process_pids("users:((sshd,fd=4))").is_empty());
    }
}
