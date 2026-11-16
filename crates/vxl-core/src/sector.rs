//! Sectors (rooms), their channels, and member roles.

use std::fmt;
use std::str::FromStr;

use data_encoding::HEXLOWER_PERMISSIVE;
use serde::{Deserialize, Serialize};

use crate::identity::parse_hex32;
use crate::{Error, PeerId};

/// Maximum Sector name length, in characters.
pub const SECTOR_NAME_MAX: usize = 48;
/// Maximum display name length, in characters.
pub const DISPLAY_NAME_MAX: usize = 32;

/// 32-byte Sector identifier. It is also the iroh-gossip topic id.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SectorId(#[serde(with = "crate::bytes::fixed")] [u8; 32]);

impl SectorId {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn short(&self) -> String {
        HEXLOWER_PERMISSIVE.encode(&self.0[..4])
    }
}

impl fmt::Display for SectorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&HEXLOWER_PERMISSIVE.encode(&self.0))
    }
}

impl fmt::Debug for SectorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SectorId({})", self.short())
    }
}

impl FromStr for SectorId {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        parse_hex32(s).map(Self)
    }
}

/// Member role. Phase 1 has only the owner and invited members; role ops arrive with the ACL log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Owner,
    Member,
}

/// A Sector as known locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sector {
    pub id: SectorId,
    pub name: String,
    pub owner: PeerId,
}

impl Sector {
    /// Creates a Sector. `id_bytes` must come from the OS RNG.
    pub fn new(id_bytes: [u8; 32], name: &str, owner: PeerId) -> Result<Self, Error> {
        Ok(Self {
            id: SectorId(id_bytes),
            name: clean_name(name, SECTOR_NAME_MAX)?,
            owner,
        })
    }

    pub fn role_of(&self, peer: &PeerId) -> Role {
        if *peer == self.owner {
            Role::Owner
        } else {
            Role::Member
        }
    }
}

/// Channel kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChannelKind {
    Text,
    Comms,
}

/// A channel inside a Sector. Phase 1 ships a fixed default set; editable channels come with the ACL log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Channel {
    pub id: u16,
    pub name: &'static str,
    pub kind: ChannelKind,
}

/// The default channels every Sector starts with.
pub const DEFAULT_CHANNELS: [Channel; 2] = [
    Channel {
        id: 1,
        name: "general",
        kind: ChannelKind::Text,
    },
    Channel {
        id: 2,
        name: "Comms 1",
        kind: ChannelKind::Comms,
    },
];

/// Trims a user-supplied name and checks it is 1..=max characters with no control characters.
pub fn clean_name(raw: &str, max: usize) -> Result<String, Error> {
    let name = raw.trim();
    let count = name.chars().count();
    if count == 0 || count > max || name.chars().any(char::is_control) {
        return Err(Error::BadName { max });
    }
    Ok(name.to_owned())
}

/// Validates a display name.
pub fn clean_display_name(raw: &str) -> Result<String, Error> {
    clean_name(raw, DISPLAY_NAME_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Identity;

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(clean_name("  Squad  ", 48), Ok("Squad".to_owned()));
        assert!(clean_name("   ", 48).is_err());
        assert!(clean_name("a\nb", 48).is_err());
        assert!(clean_name(&"x".repeat(49), 48).is_err());
        assert!(clean_name(&"é".repeat(48), 48).is_ok());
    }

    #[test]
    fn owner_role() {
        let owner = Identity::from_seed(&[1; 32]).peer_id();
        let other = Identity::from_seed(&[2; 32]).peer_id();
        let s = Sector::new([5; 32], "Ops", owner).expect("valid");
        assert_eq!(s.role_of(&owner), Role::Owner);
        assert_eq!(s.role_of(&other), Role::Member);
    }

    #[test]
    fn sector_id_hex_round_trip() {
        let id = SectorId::from_bytes([0xab; 32]);
        assert_eq!(id.to_string().parse::<SectorId>(), Ok(id));
    }
}
