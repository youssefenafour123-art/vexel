//! Invite capabilities and the `vxl://s/<base32(MessagePack)>` Sector Key codec.
//!
//! A Sector Key carries two things:
//! - an [`InviteCap`], signed by the Sector owner, which is the joiner's proof of membership. It binds
//!   the Sector id, its name, the owner and an optional expiry, so none of them can be swapped.
//! - bootstrap [`PeerAddr`]s, which are **unsigned dialing hints**. Any member can re-share the
//!   owner's cap with their own address, and a forged address can at worst fail to connect.

use std::net::SocketAddr;

use data_encoding::BASE32_NOPAD;
use serde::{Deserialize, Serialize};

use crate::{Error, Identity, PeerId, Sector, SectorId, Signature};

/// URL prefix of every Sector Key.
pub const PREFIX: &str = "vxl://s/";
/// Current Sector Key format version.
pub const KEY_VERSION: u8 = 1;
/// Upper bound on an encoded key, to reject junk before decoding.
pub const MAX_KEY_CHARS: usize = 4096;
/// Most bootstrap addresses a key may carry.
pub const MAX_BOOTSTRAP: usize = 8;
/// Most direct IP addresses kept per bootstrap peer.
pub const MAX_IPS_PER_PEER: usize = 8;

const INVITE_DOMAIN: &[u8] = b"vxl/invite/v1\0";

/// Where to reach a peer: its id plus optional relay URL and direct socket addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerAddr {
    pub id: PeerId,
    pub relay: Option<String>,
    pub ips: Vec<SocketAddr>,
}

impl PeerAddr {
    /// Drops excess direct addresses so keys stay short.
    pub fn trimmed(mut self) -> Self {
        self.ips.truncate(MAX_IPS_PER_PEER);
        self
    }
}

/// Owner-signed permission to join one Sector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteCap {
    pub sector_id: SectorId,
    pub sector_name: String,
    pub owner: PeerId,
    /// Unix milliseconds after which the cap no longer admits *new* members. `None` never expires.
    pub expires_at_ms: Option<u64>,
    /// Random per-invite value so two invites for the same Sector are distinct.
    #[serde(with = "crate::bytes::fixed")]
    pub nonce: [u8; 16],
    pub sig: Signature,
}

impl InviteCap {
    /// Issues an invite. Fails unless `owner` really is the Sector owner.
    pub fn issue(
        owner: &Identity,
        sector: &Sector,
        expires_at_ms: Option<u64>,
        nonce: [u8; 16],
    ) -> Result<Self, Error> {
        if owner.peer_id() != sector.owner {
            return Err(Error::NotOwner);
        }
        let msg = signing_bytes(
            &sector.id,
            &sector.name,
            &sector.owner,
            expires_at_ms,
            &nonce,
        )?;
        Ok(Self {
            sector_id: sector.id,
            sector_name: sector.name.clone(),
            owner: sector.owner,
            expires_at_ms,
            nonce,
            sig: owner.sign(&msg),
        })
    }

    /// Checks the owner's signature (ignores expiry).
    pub fn verify(&self) -> Result<(), Error> {
        let msg = signing_bytes(
            &self.sector_id,
            &self.sector_name,
            &self.owner,
            self.expires_at_ms,
            &self.nonce,
        )?;
        self.owner.verify(&msg, &self.sig)
    }

    /// Checks the signature and that the cap has not expired at `now_ms`.
    pub fn check(&self, now_ms: u64) -> Result<(), Error> {
        self.verify()?;
        match self.expires_at_ms {
            Some(exp) if now_ms > exp => Err(Error::Expired),
            _ => Ok(()),
        }
    }

    /// The Sector this cap admits to.
    pub fn sector(&self) -> Sector {
        Sector {
            id: self.sector_id,
            name: self.sector_name.clone(),
            owner: self.owner,
        }
    }
}

fn signing_bytes(
    id: &SectorId,
    name: &str,
    owner: &PeerId,
    expires_at_ms: Option<u64>,
    nonce: &[u8; 16],
) -> Result<Vec<u8>, Error> {
    #[derive(Serialize)]
    struct Signed<'a> {
        id: &'a SectorId,
        name: &'a str,
        owner: &'a PeerId,
        exp: Option<u64>,
        #[serde(with = "crate::bytes::fixed")]
        nonce: [u8; 16],
    }
    let body = rmp_serde::to_vec(&Signed {
        id,
        name,
        owner,
        exp: expires_at_ms,
        nonce: *nonce,
    })
    .map_err(|_| Error::Encode)?;
    let mut out = Vec::with_capacity(INVITE_DOMAIN.len() + body.len());
    out.extend_from_slice(INVITE_DOMAIN);
    out.extend_from_slice(&body);
    Ok(out)
}

/// A shareable invite: `vxl://s/...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorKey {
    pub cap: InviteCap,
    pub bootstrap: Vec<PeerAddr>,
}

#[derive(Serialize, Deserialize)]
struct KeyWire {
    v: u8,
    cap: InviteCap,
    boot: Vec<PeerAddr>,
}

impl SectorKey {
    pub fn new(cap: InviteCap, bootstrap: impl IntoIterator<Item = PeerAddr>) -> Self {
        Self {
            cap,
            bootstrap: bootstrap
                .into_iter()
                .take(MAX_BOOTSTRAP)
                .map(PeerAddr::trimmed)
                .collect(),
        }
    }

    pub fn sector_id(&self) -> SectorId {
        self.cap.sector_id
    }

    /// Encodes as `vxl://s/<lowercase base32>`.
    pub fn encode(&self) -> Result<String, Error> {
        let wire = KeyWire {
            v: KEY_VERSION,
            cap: self.cap.clone(),
            boot: self.bootstrap.clone(),
        };
        let bytes = rmp_serde::to_vec(&wire).map_err(|_| Error::Encode)?;
        let mut out = String::from(PREFIX);
        out.push_str(&BASE32_NOPAD.encode(&bytes).to_ascii_lowercase());
        Ok(out)
    }

    /// Decodes and verifies the owner signature. Expiry is **not** checked here; call
    /// [`InviteCap::check`] with the current time before joining.
    ///
    /// Accepts surrounding whitespace, any letter case and a trailing `/` (browsers add one to deep links).
    pub fn decode(raw: &str) -> Result<Self, Error> {
        let s = raw.trim();
        if s.len() > MAX_KEY_CHARS {
            return Err(Error::MalformedKey);
        }
        let lower = s.to_ascii_lowercase();
        let body = lower.strip_prefix(PREFIX).ok_or(Error::NotASectorKey)?;
        let body = body.trim_end_matches('/');
        let bytes = BASE32_NOPAD
            .decode(body.to_ascii_uppercase().as_bytes())
            .map_err(|_| Error::MalformedKey)?;
        // Peek the version first so a newer key gives a clear error instead of "malformed".
        let wire: KeyWire = match rmp_serde::from_slice(&bytes) {
            Ok(w) => w,
            Err(_) => {
                // A compact struct is a MessagePack fixarray (0x91..=0x9f) whose first element is `v`.
                return Err(match (bytes.first(), bytes.get(1)) {
                    (Some(0x91..=0x9f), Some(&v)) if (2..0x80).contains(&v) => {
                        Error::UnsupportedVersion(v)
                    }
                    _ => Error::MalformedKey,
                });
            }
        };
        if wire.v != KEY_VERSION {
            return Err(Error::UnsupportedVersion(wire.v));
        }
        if wire.boot.len() > MAX_BOOTSTRAP {
            return Err(Error::MalformedKey);
        }
        wire.cap.verify()?;
        Ok(Self {
            cap: wire.cap,
            bootstrap: wire.boot,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Identity, Sector, SectorKey) {
        let owner = Identity::from_seed(&[11; 32]);
        let sector = Sector::new([22; 32], "Night Ops", owner.peer_id()).expect("valid name");
        let cap = InviteCap::issue(&owner, &sector, Some(10_000), [3; 16]).expect("owner");
        let addr = PeerAddr {
            id: owner.peer_id(),
            relay: Some("https://relay.example/".into()),
            ips: vec!["192.168.1.20:41000".parse().expect("addr")],
        };
        let key = SectorKey::new(cap, [addr]);
        (owner, sector, key)
    }

    #[test]
    fn round_trip() {
        let (_, sector, key) = fixture();
        let s = key.encode().expect("encode");
        assert!(s.starts_with(PREFIX));
        let back = SectorKey::decode(&s).expect("decode");
        assert_eq!(back, key);
        assert_eq!(back.cap.sector(), sector);
    }

    #[test]
    fn tolerant_parsing() {
        let (_, _, key) = fixture();
        let s = key.encode().expect("encode");
        let messy = format!("  {}/ \n", s.to_ascii_uppercase());
        assert_eq!(SectorKey::decode(&messy).expect("decode"), key);
    }

    #[test]
    fn tampered_name_is_rejected() {
        let (_, _, mut key) = fixture();
        key.cap.sector_name = "Evil Ops".into();
        let s = key.encode().expect("encode");
        assert_eq!(SectorKey::decode(&s), Err(Error::BadSignature));
    }

    #[test]
    fn tampered_expiry_is_rejected() {
        let (_, _, mut key) = fixture();
        key.cap.expires_at_ms = None;
        let s = key.encode().expect("encode");
        assert_eq!(SectorKey::decode(&s), Err(Error::BadSignature));
    }

    #[test]
    fn bootstrap_is_an_unsigned_hint() {
        let (_, _, mut key) = fixture();
        key.bootstrap.clear();
        let s = key.encode().expect("encode");
        assert!(SectorKey::decode(&s).is_ok());
    }

    #[test]
    fn non_owner_cannot_issue() {
        let (_, sector, _) = fixture();
        let other = Identity::from_seed(&[99; 32]);
        assert_eq!(
            InviteCap::issue(&other, &sector, None, [0; 16]),
            Err(Error::NotOwner)
        );
    }

    #[test]
    fn expiry() {
        let (_, _, key) = fixture();
        assert_eq!(key.cap.check(10_000), Ok(()));
        assert_eq!(key.cap.check(10_001), Err(Error::Expired));
    }

    #[test]
    fn junk_is_rejected() {
        assert_eq!(SectorKey::decode("https://x"), Err(Error::NotASectorKey));
        assert_eq!(SectorKey::decode("vxl://s/!!!"), Err(Error::MalformedKey));
        assert_eq!(SectorKey::decode("vxl://s/aaaa"), Err(Error::MalformedKey));
        let huge = format!("{PREFIX}{}", "a".repeat(MAX_KEY_CHARS));
        assert_eq!(SectorKey::decode(&huge), Err(Error::MalformedKey));
    }

    #[test]
    fn typical_key_is_reasonably_short() {
        let (_, _, key) = fixture();
        let s = key.encode().expect("encode");
        assert!(s.len() < 400, "key length {}", s.len());
    }
}
