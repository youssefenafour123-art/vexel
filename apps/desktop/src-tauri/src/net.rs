//! Thin adapter between Tauri and `vxl-net` / `vxl-store`. No product logic lives here: commands
//! translate arguments, call the crates, and turn results into UI views; [`pump`] forwards
//! [`NetEvent`]s as Tauri events and persists what the store needs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use vxl_core::{MemberView, SectorId, SectorKey};
use vxl_metrics::CoreMetrics;
use vxl_net::{DEFAULT_INVITE_TTL, LinkStats, NetConfig, NetEvent, Node, RelayChoice, now_ms};
use vxl_store::{LocalProfile, SETTING_RELAY_URL, SectorRecord, Store};

/// Event names. Mirrored in `apps/desktop/src/lib/net.ts`.
pub const EVENT_ROSTER: &str = "net:roster";
pub const EVENT_LINKS: &str = "net:links";
pub const EVENT_ONLINE: &str = "net:online";

/// Running network state, managed by Tauri once startup succeeds.
pub struct Net {
    node: Node,
    store: Mutex<Store>,
    profile: Mutex<LocalProfile>,
    rosters: Mutex<HashMap<SectorId, Vec<MemberView>>>,
    online: Mutex<Option<OnlineView>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    peer_id: String,
    short_id: String,
    display_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectorView {
    id: String,
    name: String,
    is_owner: bool,
    members: Vec<MemberView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPreview {
    sector_id: String,
    sector_name: String,
    owner_short: String,
    expires_at_ms: Option<u64>,
    expired: bool,
    already_joined: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineView {
    relay: Option<String>,
    direct: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RosterEvent<'a> {
    sector_id: String,
    members: &'a [MemberView],
}

/// Opens the store, starts the node, rejoins stored Sectors and starts the event pump.
/// Must run inside the Tauri (Tokio) runtime.
pub async fn start(app: AppHandle, metrics: Arc<CoreMetrics>) -> Result<(), String> {
    let dir = app
        .path()
        .data_dir()
        .map_err(|e| e.to_string())?
        .join("vexel");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let store = Store::open(dir.join("vexel.db")).map_err(|e| e.to_string())?;
    let profile = store.load_or_create_profile().map_err(|e| e.to_string())?;
    let relay = match store
        .setting(SETTING_RELAY_URL)
        .map_err(|e| e.to_string())?
    {
        Some(url) => RelayChoice::Custom(url),
        None => RelayChoice::Default,
    };
    let (node, events) = Node::start(NetConfig {
        identity: profile.identity.clone(),
        display_name: profile.display_name.clone(),
        relay,
        metrics,
    })
    .await
    .map_err(|e| e.to_string())?;
    tracing::info!(peer = %profile.identity.peer_id().short(), "link node up");

    let records = store.sectors().map_err(|e| e.to_string())?;
    let net = Arc::new(Net {
        node,
        store: Mutex::new(store),
        profile: Mutex::new(profile),
        rosters: Mutex::default(),
        online: Mutex::default(),
    });
    app.manage(Arc::clone(&net));
    tauri::async_runtime::spawn(pump(app.clone(), Arc::clone(&net), events));

    for rec in records {
        if let Err(err) = rejoin(&net, &rec).await {
            tracing::warn!(sector = %rec.sector.id.short(), %err, "rejoin failed");
        }
    }
    Ok(())
}

async fn rejoin(net: &Net, rec: &SectorRecord) -> Result<(), String> {
    let (boot, known) = {
        let store = lock(&net.store);
        let members = store.members(&rec.sector.id).map_err(|e| e.to_string())?;
        let mut boot = Vec::new();
        for m in &members {
            if let Some(a) = store.peer_addr(&m.peer).map_err(|e| e.to_string())? {
                boot.push(a);
            }
        }
        if let Some(cap) = &rec.cap
            && let Some(a) = store.peer_addr(&cap.owner).map_err(|e| e.to_string())?
            && !boot.iter().any(|b| b.id == a.id)
        {
            boot.push(a);
        }
        let known = members
            .into_iter()
            .map(|m| (m.peer, m.display_name))
            .collect();
        (boot, known)
    };
    net.node
        .join(rec.sector.clone(), rec.cap.clone(), boot, known)
        .await
        .map_err(|e| e.to_string())
}

async fn pump(app: AppHandle, net: Arc<Net>, mut events: mpsc::Receiver<NetEvent>) {
    while let Some(ev) = events.recv().await {
        match ev {
            NetEvent::Online { addr } => {
                let view = OnlineView {
                    relay: addr.relay,
                    direct: addr.ips.len(),
                };
                *lock(&net.online) = Some(view.clone());
                let _ = app.emit(EVENT_ONLINE, view);
            }
            NetEvent::Roster { sector_id, members } => {
                // Cache first, so a `sectors_list` racing this event never sees an older roster.
                lock(&net.rosters).insert(sector_id, members.clone());
                let _ = app.emit(
                    EVENT_ROSTER,
                    RosterEvent {
                        sector_id: sector_id.to_string(),
                        members: &members,
                    },
                );
            }
            NetEvent::MemberSeen {
                sector_id,
                addr,
                display_name,
                seen_ms,
            } => {
                let store = lock(&net.store);
                if let Err(err) = store
                    .upsert_member(&sector_id, &addr.id, &display_name, seen_ms)
                    .and_then(|()| store.upsert_peer_addr(&addr, seen_ms))
                {
                    tracing::warn!(%err, "could not persist member");
                }
            }
            NetEvent::Links(links) => {
                let _ = app.emit::<&[LinkStats]>(EVENT_LINKS, &links);
            }
        }
    }
}

fn net(app: &AppHandle) -> Result<Arc<Net>, String> {
    app.try_state::<Arc<Net>>()
        .map(|s| Arc::clone(&s))
        .ok_or_else(|| "LINK OFFLINE // NODE NOT RUNNING".to_owned())
}

fn profile_view(p: &LocalProfile) -> ProfileView {
    let id = p.identity.peer_id();
    ProfileView {
        peer_id: id.to_string(),
        short_id: id.short(),
        display_name: p.display_name.clone(),
    }
}

impl Net {
    fn sector_view(&self, rec: &SectorRecord) -> SectorView {
        let me = lock(&self.profile).identity.peer_id();
        SectorView {
            id: rec.sector.id.to_string(),
            name: rec.sector.name.clone(),
            is_owner: rec.sector.owner == me,
            members: lock(&self.rosters)
                .get(&rec.sector.id)
                .cloned()
                .unwrap_or_default(),
        }
    }

    fn record(&self, id: &str) -> Result<SectorRecord, String> {
        let id: SectorId = id.parse().map_err(|e: vxl_core::Error| e.to_string())?;
        lock(&self.store)
            .sector(&id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "UNKNOWN SECTOR".to_owned())
    }
}

#[tauri::command]
pub async fn profile_get(app: AppHandle) -> Result<ProfileView, String> {
    let net = net(&app)?;
    let view = profile_view(&lock(&net.profile));
    Ok(view)
}

#[tauri::command]
pub async fn profile_set_name(app: AppHandle, name: String) -> Result<ProfileView, String> {
    let net = net(&app)?;
    let clean = net
        .node
        .set_display_name(&name)
        .map_err(|e| e.to_string())?;
    lock(&net.store)
        .set_display_name(&clean)
        .map_err(|e| e.to_string())?;
    let mut p = lock(&net.profile);
    p.display_name = clean;
    Ok(profile_view(&p))
}

#[tauri::command]
pub async fn link_status(app: AppHandle) -> Result<Option<OnlineView>, String> {
    let net = net(&app)?;
    let view = lock(&net.online).clone();
    Ok(view)
}

#[tauri::command]
pub async fn sectors_list(app: AppHandle) -> Result<Vec<SectorView>, String> {
    let net = net(&app)?;
    let recs = lock(&net.store).sectors().map_err(|e| e.to_string())?;
    Ok(recs.iter().map(|r| net.sector_view(r)).collect())
}

#[tauri::command]
pub async fn sector_create(app: AppHandle, name: String) -> Result<SectorView, String> {
    let net = net(&app)?;
    let identity = lock(&net.profile).identity.clone();
    let sector = vxl_net::new_sector(&identity, &name).map_err(|e| e.to_string())?;
    let rec = SectorRecord {
        sector: sector.clone(),
        cap: None,
        joined_at_ms: now_ms(),
    };
    lock(&net.store)
        .upsert_sector(&rec)
        .map_err(|e| e.to_string())?;
    net.node
        .join(sector, None, vec![], vec![])
        .await
        .map_err(|e| e.to_string())?;
    Ok(net.sector_view(&rec))
}

#[tauri::command]
pub async fn sector_key_preview(app: AppHandle, key: String) -> Result<KeyPreview, String> {
    let net = net(&app)?;
    let key = SectorKey::decode(&key).map_err(|e| e.to_string())?;
    let already_joined = lock(&net.store)
        .sector(&key.sector_id())
        .map_err(|e| e.to_string())?
        .is_some();
    Ok(KeyPreview {
        sector_id: key.sector_id().to_string(),
        sector_name: key.cap.sector_name.clone(),
        owner_short: key.cap.owner.short(),
        expires_at_ms: key.cap.expires_at_ms,
        expired: key.cap.check(now_ms()).is_err(),
        already_joined,
    })
}

#[tauri::command]
pub async fn sector_join(app: AppHandle, key: String) -> Result<SectorView, String> {
    let net = net(&app)?;
    let key = SectorKey::decode(&key).map_err(|e| e.to_string())?;
    key.cap.check(now_ms()).map_err(|e| e.to_string())?;
    let sector = key.cap.sector();
    if lock(&net.store)
        .sector(&sector.id)
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Err("SECTOR ALREADY LINKED".to_owned());
    }
    let rec = SectorRecord {
        sector: sector.clone(),
        cap: Some(key.cap.clone()),
        joined_at_ms: now_ms(),
    };
    lock(&net.store)
        .upsert_sector(&rec)
        .map_err(|e| e.to_string())?;
    if let Err(err) = net
        .node
        .join(sector.clone(), Some(key.cap), key.bootstrap, vec![])
        .await
    {
        let _ = lock(&net.store).remove_sector(&sector.id);
        return Err(err.to_string());
    }
    Ok(net.sector_view(&rec))
}

#[tauri::command]
pub async fn sector_key(app: AppHandle, sector_id: String) -> Result<String, String> {
    let net = net(&app)?;
    let rec = net.record(&sector_id)?;
    net.node
        .sector_key(&rec.sector, rec.cap.as_ref(), Some(DEFAULT_INVITE_TTL))
        .and_then(|k| Ok(k.encode()?))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sector_leave(app: AppHandle, sector_id: String) -> Result<(), String> {
    let net = net(&app)?;
    let rec = net.record(&sector_id)?;
    let _ = net.node.leave(&rec.sector.id).await;
    lock(&net.rosters).remove(&rec.sector.id);
    lock(&net.store)
        .remove_sector(&rec.sector.id)
        .map_err(|e| e.to_string())
}

/// Leaves every Sector gracefully so peers see us go offline at once.
pub async fn shutdown(app: &AppHandle) {
    if let Some(net) = app.try_state::<Arc<Net>>() {
        net.node.shutdown().await;
    }
}
