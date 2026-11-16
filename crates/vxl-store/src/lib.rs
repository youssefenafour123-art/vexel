//! # vxl-store
//!
//! SQLite persistence for the local peer: identity, joined Sectors, known members, last known peer
//! addresses (bootstrap hints for the next start) and settings. Phase 2 adds the signed message log.
//!
//! The API is synchronous and fast (single-row statements). Async callers should hold the [`Store`]
//! behind a mutex and avoid holding the lock across `.await`.
//!
//! **Caveat:** the identity seed is stored unencrypted in the database file, under the user's profile
//! directory. Wrapping it with Windows DPAPI (through `vxl-platform`) is planned.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use vxl_core::identity::SEED_LEN;
use vxl_core::{Identity, InviteCap, PeerAddr, PeerId, Sector, SectorId};

/// Errors from the store.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("OS random number generator failed")]
    Rng,
    #[error("stored data is corrupt: {0}")]
    Corrupt(&'static str),
    #[error("database schema version {0} is newer than this build")]
    FutureSchema(i64),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Settings key for a custom relay URL.
pub const SETTING_RELAY_URL: &str = "net.relay_url";

const SCHEMA_VERSION: i64 = 1;

const MIGRATION_1: &str = "
CREATE TABLE identity (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    seed          BLOB NOT NULL,
    display_name  TEXT NOT NULL
);
CREATE TABLE sectors (
    id            BLOB PRIMARY KEY,
    name          TEXT NOT NULL,
    owner         BLOB NOT NULL,
    cap           BLOB,
    joined_at_ms  INTEGER NOT NULL
);
CREATE TABLE members (
    sector_id     BLOB NOT NULL REFERENCES sectors(id) ON DELETE CASCADE,
    peer_id       BLOB NOT NULL,
    display_name  TEXT NOT NULL,
    last_seen_ms  INTEGER NOT NULL,
    PRIMARY KEY (sector_id, peer_id)
);
CREATE TABLE peer_addrs (
    peer_id       BLOB PRIMARY KEY,
    addr          BLOB NOT NULL,
    updated_ms    INTEGER NOT NULL
);
CREATE TABLE settings (
    key           TEXT PRIMARY KEY,
    value         TEXT NOT NULL
);
";

/// The local install's identity and profile.
#[derive(Debug, Clone)]
pub struct LocalProfile {
    pub identity: Identity,
    pub display_name: String,
}

/// A joined Sector as persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorRecord {
    pub sector: Sector,
    /// The invite we joined with (`None` if we own the Sector).
    pub cap: Option<InviteCap>,
    pub joined_at_ms: u64,
}

/// A member we have admitted before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRecord {
    pub peer: PeerId,
    pub display_name: String,
    pub last_seen_ms: u64,
}

/// Handle to the SQLite database.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) the database at `path` and runs migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// In-memory database, for tests and throwaway CLI peers.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;",
        )?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(Error::FutureSchema(version));
        }
        if version < 1 {
            conn.execute_batch(&format!(
                "BEGIN; {MIGRATION_1} PRAGMA user_version = 1; COMMIT;"
            ))?;
        }
        Ok(Self { conn })
    }

    /// Loads the install identity, generating and saving one on first run.
    pub fn load_or_create_profile(&self) -> Result<LocalProfile> {
        let row: Option<(Vec<u8>, String)> = self
            .conn
            .query_row(
                "SELECT seed, display_name FROM identity WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((seed, display_name)) = row {
            let seed: [u8; SEED_LEN] = seed
                .try_into()
                .map_err(|_| Error::Corrupt("identity seed length"))?;
            return Ok(LocalProfile {
                identity: Identity::from_seed(&seed),
                display_name,
            });
        }
        let mut seed = [0u8; SEED_LEN];
        getrandom::fill(&mut seed).map_err(|_| Error::Rng)?;
        let identity = Identity::from_seed(&seed);
        let display_name = default_display_name(&identity.peer_id());
        self.conn.execute(
            "INSERT INTO identity (id, seed, display_name) VALUES (1, ?1, ?2)",
            params![&seed[..], display_name],
        )?;
        Ok(LocalProfile {
            identity,
            display_name,
        })
    }

    /// Saves a new display name. The caller validates it first (`vxl_core::sector::clean_display_name`).
    pub fn set_display_name(&self, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE identity SET display_name = ?1 WHERE id = 1",
            params![name],
        )?;
        Ok(())
    }

    /// Inserts or updates a joined Sector.
    pub fn upsert_sector(&self, rec: &SectorRecord) -> Result<()> {
        let cap = rec
            .cap
            .as_ref()
            .map(rmp_serde::to_vec)
            .transpose()
            .map_err(|_| Error::Corrupt("cap encode"))?;
        self.conn.execute(
            "INSERT INTO sectors (id, name, owner, cap, joined_at_ms) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, cap = COALESCE(sectors.cap, excluded.cap)",
            params![
                &rec.sector.id.as_bytes()[..],
                rec.sector.name,
                &rec.sector.owner.as_bytes()[..],
                cap,
                to_i64(rec.joined_at_ms),
            ],
        )?;
        Ok(())
    }

    /// All joined Sectors, oldest first.
    pub fn sectors(&self) -> Result<Vec<SectorRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, owner, cap, joined_at_ms FROM sectors ORDER BY joined_at_ms, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, Option<Vec<u8>>>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, owner, cap, joined) = row?;
            let cap = cap
                .map(|c| rmp_serde::from_slice::<InviteCap>(&c))
                .transpose()
                .map_err(|_| Error::Corrupt("cap decode"))?;
            out.push(SectorRecord {
                sector: Sector {
                    id: SectorId::from_bytes(bytes32(&id, "sector id")?),
                    name,
                    owner: peer(&owner)?,
                },
                cap,
                joined_at_ms: from_i64(joined),
            });
        }
        Ok(out)
    }

    /// Looks up one Sector.
    pub fn sector(&self, id: &SectorId) -> Result<Option<SectorRecord>> {
        Ok(self.sectors()?.into_iter().find(|r| r.sector.id == *id))
    }

    /// Forgets a Sector and its members.
    pub fn remove_sector(&self, id: &SectorId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM sectors WHERE id = ?1",
            params![&id.as_bytes()[..]],
        )?;
        Ok(())
    }

    /// Records that `peer` is an admitted member of `sector`.
    pub fn upsert_member(
        &self,
        sector: &SectorId,
        peer: &PeerId,
        display_name: &str,
        last_seen_ms: u64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO members (sector_id, peer_id, display_name, last_seen_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(sector_id, peer_id) DO UPDATE SET
                display_name = excluded.display_name,
                last_seen_ms = MAX(members.last_seen_ms, excluded.last_seen_ms)",
            params![
                &sector.as_bytes()[..],
                &peer.as_bytes()[..],
                display_name,
                to_i64(last_seen_ms)
            ],
        )?;
        Ok(())
    }

    /// Members of a Sector, most recently seen first.
    pub fn members(&self, sector: &SectorId) -> Result<Vec<MemberRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT peer_id, display_name, last_seen_ms FROM members WHERE sector_id = ?1
             ORDER BY last_seen_ms DESC",
        )?;
        let rows = stmt.query_map(params![&sector.as_bytes()[..]], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (p, display_name, seen) = row?;
            out.push(MemberRecord {
                peer: peer(&p)?,
                display_name,
                last_seen_ms: from_i64(seen),
            });
        }
        Ok(out)
    }

    /// Remembers a peer's latest address, used as a bootstrap hint on the next start.
    pub fn upsert_peer_addr(&self, addr: &PeerAddr, now_ms: u64) -> Result<()> {
        let blob = rmp_serde::to_vec(addr).map_err(|_| Error::Corrupt("addr encode"))?;
        self.conn.execute(
            "INSERT INTO peer_addrs (peer_id, addr, updated_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(peer_id) DO UPDATE SET addr = excluded.addr, updated_ms = excluded.updated_ms",
            params![&addr.id.as_bytes()[..], blob, to_i64(now_ms)],
        )?;
        Ok(())
    }

    /// The last known address of a peer.
    pub fn peer_addr(&self, peer: &PeerId) -> Result<Option<PeerAddr>> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT addr FROM peer_addrs WHERE peer_id = ?1",
                params![&peer.as_bytes()[..]],
                |r| r.get(0),
            )
            .optional()?;
        blob.map(|b| rmp_serde::from_slice(&b).map_err(|_| Error::Corrupt("addr decode")))
            .transpose()
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Sets (`Some`) or clears (`None`) a setting.
    pub fn set_setting(&self, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(v) => self.conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, v],
            )?,
            None => self
                .conn
                .execute("DELETE FROM settings WHERE key = ?1", params![key])?,
        };
        Ok(())
    }
}

/// `OPERATOR-1a2b`, from the first 4 hex characters of the peer id.
pub fn default_display_name(peer: &PeerId) -> String {
    let short = peer.short();
    format!(
        "OPERATOR-{}",
        short.get(..4).unwrap_or(&short).to_uppercase()
    )
}

fn bytes32(v: &[u8], what: &'static str) -> Result<[u8; 32]> {
    v.try_into().map_err(|_| Error::Corrupt(what))
}

fn peer(v: &[u8]) -> Result<PeerId> {
    PeerId::from_bytes(bytes32(v, "peer id")?).map_err(|_| Error::Corrupt("peer id"))
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn from_i64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sector_of(owner: &Identity) -> Sector {
        Sector::new([4; 32], "Ops", owner.peer_id()).expect("name")
    }

    #[test]
    fn profile_is_created_once_and_reloaded() {
        let dir = std::env::temp_dir().join(format!("vxl-store-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("t.db");
        let _ = std::fs::remove_file(&path);
        let first = Store::open(&path)
            .expect("open")
            .load_or_create_profile()
            .expect("p");
        let store = Store::open(&path).expect("reopen");
        let second = store.load_or_create_profile().expect("p");
        assert_eq!(first.identity.peer_id(), second.identity.peer_id());
        assert!(second.display_name.starts_with("OPERATOR-"));
        store.set_display_name("ace").expect("set");
        assert_eq!(
            store.load_or_create_profile().expect("p").display_name,
            "ace"
        );
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sectors_and_members_round_trip() {
        let store = Store::open_in_memory().expect("open");
        let owner = Identity::from_seed(&[1; 32]);
        let sector = sector_of(&owner);
        let cap = InviteCap::issue(&owner, &sector, None, [2; 16]).expect("cap");
        let rec = SectorRecord {
            sector: sector.clone(),
            cap: Some(cap),
            joined_at_ms: 10,
        };
        store.upsert_sector(&rec).expect("upsert");
        assert_eq!(store.sectors().expect("list"), vec![rec.clone()]);

        let m = Identity::from_seed(&[3; 32]).peer_id();
        store.upsert_member(&sector.id, &m, "ace", 5).expect("m");
        store.upsert_member(&sector.id, &m, "ace2", 3).expect("m");
        let members = store.members(&sector.id).expect("members");
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].display_name, "ace2");
        assert_eq!(members[0].last_seen_ms, 5);

        store.remove_sector(&sector.id).expect("rm");
        assert!(store.sectors().expect("list").is_empty());
        assert!(store.members(&sector.id).expect("members").is_empty());
    }

    #[test]
    fn peer_addrs_and_settings() {
        let store = Store::open_in_memory().expect("open");
        let id = Identity::from_seed(&[1; 32]).peer_id();
        let addr = PeerAddr {
            id,
            relay: Some("https://r.example/".into()),
            ips: vec!["10.0.0.2:1234".parse().expect("ip")],
        };
        store.upsert_peer_addr(&addr, 1).expect("addr");
        assert_eq!(store.peer_addr(&id).expect("get"), Some(addr));

        assert_eq!(store.setting(SETTING_RELAY_URL).expect("get"), None);
        store
            .set_setting(SETTING_RELAY_URL, Some("https://x/"))
            .expect("set");
        assert_eq!(
            store.setting(SETTING_RELAY_URL).expect("get").as_deref(),
            Some("https://x/")
        );
        store.set_setting(SETTING_RELAY_URL, None).expect("clear");
        assert_eq!(store.setting(SETTING_RELAY_URL).expect("get"), None);
    }
}
