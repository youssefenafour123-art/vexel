//! Conversions between `vxl-core` types and iroh types. `vxl-core` stays free of iroh.

use iroh::{EndpointAddr, EndpointId, PublicKey, RelayUrl, TransportAddr};
use vxl_core::{PeerAddr, PeerId, SectorId};

pub(crate) fn endpoint_id(peer: &PeerId) -> Option<EndpointId> {
    PublicKey::from_bytes(peer.as_bytes()).ok()
}

pub(crate) fn peer_id(id: &EndpointId) -> Option<PeerId> {
    PeerId::from_bytes(*id.as_bytes()).ok()
}

pub(crate) fn topic(sector: &SectorId) -> iroh_gossip::TopicId {
    iroh_gossip::TopicId::from_bytes(*sector.as_bytes())
}

pub(crate) fn to_endpoint_addr(addr: &PeerAddr) -> Option<EndpointAddr> {
    let mut out = EndpointAddr::new(endpoint_id(&addr.id)?);
    if let Some(url) = addr
        .relay
        .as_deref()
        .and_then(|r| r.parse::<RelayUrl>().ok())
    {
        out = out.with_relay_url(url);
    }
    for ip in &addr.ips {
        out = out.with_ip_addr(*ip);
    }
    Some(out)
}

pub(crate) fn from_endpoint_addr(addr: &EndpointAddr) -> Option<PeerAddr> {
    let relay = addr.addrs.iter().find_map(|t| match t {
        TransportAddr::Relay(url) => Some(url.to_string()),
        _ => None,
    });
    Some(
        PeerAddr {
            id: peer_id(&addr.id)?,
            relay,
            ips: addr.ip_addrs().copied().collect(),
        }
        .trimmed(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use vxl_core::Identity;

    #[test]
    fn addr_round_trip() {
        let id = Identity::from_seed(&[8; 32]).peer_id();
        let a = PeerAddr {
            id,
            relay: Some("https://relay.example.org./".into()),
            ips: vec!["10.1.2.3:4000".parse().expect("ip")],
        };
        let e = to_endpoint_addr(&a).expect("to");
        let back = from_endpoint_addr(&e).expect("from");
        assert_eq!(back.id, a.id);
        assert_eq!(back.ips, a.ips);
        assert!(back.relay.is_some());
    }
}
