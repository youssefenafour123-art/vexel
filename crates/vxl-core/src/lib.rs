//! # vxl-core
//!
//! VEXEL domain model and brand constants. This crate is pure logic: **no I/O and no async runtime**.
//! Anything that needs the clock or randomness takes it as an argument (`now_ms`, seeds, nonces), so
//! every rule here is deterministic and unit-testable.
//!
//! - [`identity`]: ed25519 install identity ([`Identity`], [`PeerId`], [`Signature`]).
//! - [`sector`]: [`Sector`], [`SectorId`], roles, channels and name rules.
//! - [`key`]: owner-signed [`InviteCap`] and the `vxl://s/...` [`SectorKey`] codec.
//! - [`wire`]: signed MessagePack gossip envelopes (control plane).
//! - [`roster`]: presence-driven membership with the Phase 1 ACL (owner + invite cap).

pub mod brand;
mod bytes;
pub mod error;
pub mod identity;
pub mod key;
pub mod roster;
pub mod sector;
pub mod wire;

pub use error::Error;
pub use identity::{Identity, PeerId, Signature};
pub use key::{InviteCap, PeerAddr, SectorKey};
pub use roster::{MemberView, Roster, RosterChange};
pub use sector::{Channel, ChannelKind, Role, Sector, SectorId};
