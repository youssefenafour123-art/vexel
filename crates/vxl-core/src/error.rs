//! The crate-wide error type.

/// Errors from `vxl-core` codecs and rules.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid hex identifier")]
    BadHex,
    #[error("invalid public key")]
    BadPublicKey,
    #[error("signature verification failed")]
    BadSignature,
    #[error("name must be 1-{max} visible characters")]
    BadName { max: usize },
    #[error("not a Sector Key (expected vxl://s/...)")]
    NotASectorKey,
    #[error("Sector Key is malformed")]
    MalformedKey,
    #[error("Sector Key version {0} is not supported by this build")]
    UnsupportedVersion(u8),
    #[error("Sector Key has expired")]
    Expired,
    #[error("only the Sector owner can issue invites")]
    NotOwner,
    #[error("message encoding failed")]
    Encode,
    #[error("message decoding failed")]
    Decode,
    #[error("message too large ({0} bytes)")]
    TooLarge(usize),
    #[error("message belongs to a different Sector")]
    WrongSector,
    #[error("presence address does not match its author")]
    AddrMismatch,
    #[error("peer is not authorized for this Sector")]
    Unauthorized,
}
