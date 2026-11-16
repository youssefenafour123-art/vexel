//! Control-plane wire format: MessagePack bodies inside a signed envelope.
//!
//! iroh-gossip messages have no author and no signature, so every gossip payload is sealed here:
//! `Envelope { author, sig, body }`, where `sig` covers a domain tag plus `body`. The body names its
//! Sector, so a message can't be replayed into a different Sector.

use serde::{Deserialize, Serialize};

use crate::{Error, Identity, InviteCap, PeerAddr, PeerId, SectorId, Signature};

/// Largest envelope accepted (iroh-gossip's default max message size is 4 KiB).
pub const MAX_ENVELOPE: usize = 4096;

const GOSSIP_DOMAIN: &[u8] = b"vxl/gossip/v1\0";

#[derive(Serialize, Deserialize)]
struct Envelope {
    author: PeerId,
    sig: Signature,
    #[serde(with = "crate::bytes::vec")]
    body: Vec<u8>,
}

/// One control message, scoped to a Sector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GossipMsg {
    pub sector_id: SectorId,
    /// Sender's clock in Unix ms. Used only to drop stale or replayed messages from the same author.
    pub ts_ms: u64,
    pub kind: GossipKind,
}

/// Control message kinds. Phase 2+ adds chat, typing and queue ops here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GossipKind {
    /// "I'm here": sent on join, every few seconds, and when a new neighbor appears.
    Presence(Box<Presence>),
    /// Graceful leave.
    Leave,
}

/// Presence payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presence {
    pub display_name: String,
    /// The author's current dialing address. `addr.id` must equal the envelope author.
    pub addr: PeerAddr,
    /// The invite the author joined with. `None` for the owner.
    pub cap: Option<InviteCap>,
}

fn signed_bytes(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(GOSSIP_DOMAIN.len() + body.len());
    out.extend_from_slice(GOSSIP_DOMAIN);
    out.extend_from_slice(body);
    out
}

/// Encodes and signs a message.
pub fn seal(identity: &Identity, msg: &GossipMsg) -> Result<Vec<u8>, Error> {
    let body = rmp_serde::to_vec_named(msg).map_err(|_| Error::Encode)?;
    let env = Envelope {
        author: identity.peer_id(),
        sig: identity.sign(&signed_bytes(&body)),
        body,
    };
    let out = rmp_serde::to_vec(&env).map_err(|_| Error::Encode)?;
    if out.len() > MAX_ENVELOPE {
        return Err(Error::TooLarge(out.len()));
    }
    Ok(out)
}

/// Verifies and decodes a message, returning its author.
pub fn open(bytes: &[u8]) -> Result<(PeerId, GossipMsg), Error> {
    if bytes.len() > MAX_ENVELOPE {
        return Err(Error::TooLarge(bytes.len()));
    }
    let env: Envelope = rmp_serde::from_slice(bytes).map_err(|_| Error::Decode)?;
    env.author.verify(&signed_bytes(&env.body), &env.sig)?;
    let msg = rmp_serde::from_slice(&env.body).map_err(|_| Error::Decode)?;
    Ok((env.author, msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presence(id: &Identity) -> GossipMsg {
        GossipMsg {
            sector_id: SectorId::from_bytes([1; 32]),
            ts_ms: 42,
            kind: GossipKind::Presence(Box::new(Presence {
                display_name: "ace".into(),
                addr: PeerAddr {
                    id: id.peer_id(),
                    relay: None,
                    ips: vec![],
                },
                cap: None,
            })),
        }
    }

    #[test]
    fn seal_open_round_trip() {
        let id = Identity::from_seed(&[4; 32]);
        let msg = presence(&id);
        let bytes = seal(&id, &msg).expect("seal");
        assert_eq!(open(&bytes), Ok((id.peer_id(), msg)));
    }

    #[test]
    fn flipped_byte_fails() {
        let id = Identity::from_seed(&[4; 32]);
        let mut bytes = seal(&id, &presence(&id)).expect("seal");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        assert!(open(&bytes).is_err());
    }

    #[test]
    fn junk_fails() {
        assert_eq!(open(b"nope"), Err(Error::Decode));
        assert_eq!(
            open(&[0u8; MAX_ENVELOPE + 1]),
            Err(Error::TooLarge(MAX_ENVELOPE + 1))
        );
    }
}
