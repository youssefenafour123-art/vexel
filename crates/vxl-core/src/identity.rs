//! Install identity: one ed25519 keypair per install. The public half is the [`PeerId`], which is
//! byte-for-byte the iroh `EndpointId`, so the identity users see and the transport identity are the same.

use std::fmt;
use std::str::FromStr;

use data_encoding::HEXLOWER_PERMISSIVE;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::Error;

/// Length of the secret seed in bytes.
pub const SEED_LEN: usize = 32;

/// A peer's public identity (ed25519 public key).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PeerId(#[serde(with = "crate::bytes::fixed")] [u8; 32]);

impl PeerId {
    /// Wraps raw public key bytes, checking that they are a valid curve point.
    pub fn from_bytes(bytes: [u8; 32]) -> Result<Self, Error> {
        VerifyingKey::from_bytes(&bytes).map_err(|_| Error::BadPublicKey)?;
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// First 8 hex characters, for HUD display.
    pub fn short(&self) -> String {
        HEXLOWER_PERMISSIVE.encode(&self.0[..4])
    }

    /// Verifies `sig` over `msg` with strict (non-malleable) ed25519 rules.
    pub fn verify(&self, msg: &[u8], sig: &Signature) -> Result<(), Error> {
        let key = VerifyingKey::from_bytes(&self.0).map_err(|_| Error::BadPublicKey)?;
        let sig = ed25519_dalek::Signature::from_bytes(&sig.0);
        key.verify_strict(msg, &sig)
            .map_err(|_| Error::BadSignature)
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&HEXLOWER_PERMISSIVE.encode(&self.0))
    }
}

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PeerId({})", self.short())
    }
}

impl FromStr for PeerId {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        Self::from_bytes(parse_hex32(s)?)
    }
}

pub(crate) fn parse_hex32(s: &str) -> Result<[u8; 32], Error> {
    let v = HEXLOWER_PERMISSIVE
        .decode(s.trim().as_bytes())
        .map_err(|_| Error::BadHex)?;
    v.try_into().map_err(|_| Error::BadHex)
}

/// A 64-byte ed25519 signature.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature(#[serde(with = "crate::bytes::fixed")] [u8; 64]);

impl Signature {
    pub const fn to_bytes(&self) -> [u8; 64] {
        self.0
    }
    pub const fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Signature(..)")
    }
}

/// The local install's signing identity. The seed is secret: `Debug` never prints it.
#[derive(Clone)]
pub struct Identity {
    key: SigningKey,
}

impl Identity {
    /// Builds the identity from a 32-byte secret seed. Callers generate the seed from the OS RNG.
    pub fn from_seed(seed: &[u8; SEED_LEN]) -> Self {
        Self {
            key: SigningKey::from_bytes(seed),
        }
    }

    /// The secret seed, for persistence only.
    pub fn seed(&self) -> [u8; SEED_LEN] {
        self.key.to_bytes()
    }

    pub fn peer_id(&self) -> PeerId {
        PeerId(self.key.verifying_key().to_bytes())
    }

    pub fn sign(&self, msg: &[u8]) -> Signature {
        Signature(self.key.sign(msg).to_bytes())
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Identity({})", self.peer_id().short())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_round_trip() {
        let id = Identity::from_seed(&[7; 32]);
        let sig = id.sign(b"hello");
        assert_eq!(id.peer_id().verify(b"hello", &sig), Ok(()));
        assert_eq!(
            id.peer_id().verify(b"hellp", &sig),
            Err(Error::BadSignature)
        );
    }

    #[test]
    fn seed_round_trip_keeps_peer_id() {
        let id = Identity::from_seed(&[9; 32]);
        let again = Identity::from_seed(&id.seed());
        assert_eq!(id.peer_id(), again.peer_id());
    }

    #[test]
    fn peer_id_hex_round_trip() {
        let p = Identity::from_seed(&[1; 32]).peer_id();
        let s = p.to_string();
        assert_eq!(s.len(), 64);
        assert_eq!(s.parse::<PeerId>(), Ok(p));
        assert_eq!(p.short().len(), 8);
    }

    #[test]
    fn debug_hides_seed() {
        let id = Identity::from_seed(&[3; 32]);
        assert!(format!("{id:?}").starts_with("Identity("));
    }
}
