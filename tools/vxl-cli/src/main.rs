//! `vxl`: headless VEXEL peer.
//!
//! Lets you run several real peers on one machine to test P2P flows. Each `--profile` gets its own
//! identity and database under `--data` (default `.vxl/`):
//!
//! ```text
//! vxl --profile a create "Night Ops" --key-out key.txt   # prints a Sector Key, stays online
//! vxl --profile b join (Get-Content key.txt)             # joins, prints members and live ping
//! vxl --profile b up                                     # rejoins every stored Sector
//! vxl --profile b sectors                                # lists stored Sectors
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use tokio::sync::mpsc;
use vxl_core::{SectorKey, brand};
use vxl_metrics::CoreMetrics;
use vxl_net::{DEFAULT_INVITE_TTL, NetConfig, NetEvent, Node, RelayChoice, now_ms};
use vxl_store::{SETTING_RELAY_URL, SectorRecord, Store};

#[derive(Debug, Parser)]
#[command(name = "vxl", version, about = brand::OS_TAG)]
struct Cli {
    /// Directory holding per-profile databases.
    #[arg(long, global = true, default_value = ".vxl")]
    data: PathBuf,
    /// Profile name: one identity per profile.
    #[arg(long, global = true, default_value = "default")]
    profile: String,
    /// Display name to announce (saved to the profile).
    #[arg(long, global = true)]
    name: Option<String>,
    /// Use this relay instead of n0's public relays.
    #[arg(long, global = true, conflicts_with = "no_relay")]
    relay: Option<String>,
    /// Disable relays (direct connections only).
    #[arg(long, global = true)]
    no_relay: bool,
    /// Exit after this many seconds (for scripted tests). Runs until Ctrl-C otherwise.
    #[arg(long = "for", global = true, value_name = "SECS")]
    run_for: Option<u64>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print build and brand info.
    Info,
    /// Print this process's working set (sanity check for the Zero-Drag RAM probe).
    Mem,
    /// Print this profile's peer id and display name.
    Whoami,
    /// List stored Sectors.
    Sectors,
    /// Create a Sector, print its Sector Key, and stay online.
    Create {
        /// Sector name.
        #[arg(value_name = "SECTOR_NAME")]
        sector_name: String,
        /// Write the Sector Key to this file too.
        #[arg(long)]
        key_out: Option<PathBuf>,
        /// Invite never expires (default: 7 days).
        #[arg(long)]
        no_expiry: bool,
    },
    /// Join a Sector from a `vxl://s/...` key and stay online.
    Join { key: String },
    /// Rejoin every stored Sector and stay online.
    Up {
        /// Write the Sector Key of the first stored Sector to this file.
        #[arg(long)]
        key_out: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Info => {
            println!("{}", brand::OS_TAG);
            println!("{}", brand::TAGLINE);
            println!("{}", brand::hud(["VERSION", brand::VERSION]));
            println!(
                "{}",
                brand::hud(["SCHEME", &format!("{}://", brand::URL_SCHEME)])
            );
            Ok(())
        }
        Command::Mem => {
            let mb = vxl_platform::memory::working_set_bytes().map_or_else(
                || "--".to_owned(),
                |b| format!("{:.1} MB", b as f64 / 1_048_576.0),
            );
            println!("{}", brand::hud(["CORE", &mb]));
            Ok(())
        }
        _ => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "warn".into()),
                )
                .with_writer(std::io::stderr)
                .init();
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("tokio runtime")?
                .block_on(run(cli))
        }
    }
}

fn open_store(cli: &Cli) -> Result<Store> {
    let dir = cli.data.join(&cli.profile);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(Store::open(dir.join("vexel.db"))?)
}

async fn run(cli: Cli) -> Result<()> {
    let store = open_store(&cli)?;
    let mut profile = store.load_or_create_profile()?;
    if let Some(name) = &cli.name {
        let clean = vxl_core::sector::clean_display_name(name)?;
        store.set_display_name(&clean)?;
        profile.display_name = clean;
    }
    let me = profile.identity.peer_id();

    match &cli.command {
        Command::Whoami => {
            println!("{}", brand::hud(["PEER", &me.to_string()]));
            println!("{}", brand::hud(["NAME", &profile.display_name]));
            return Ok(());
        }
        Command::Sectors => {
            for r in store.sectors()? {
                let role = if r.sector.owner == me {
                    "OWNER"
                } else {
                    "MEMBER"
                };
                println!(
                    "{}",
                    brand::hud([r.sector.id.to_string().as_str(), &r.sector.name, role])
                );
            }
            return Ok(());
        }
        _ => {}
    }

    let relay = if cli.no_relay {
        RelayChoice::Disabled
    } else if let Some(url) = cli.relay.clone().or(store.setting(SETTING_RELAY_URL)?) {
        RelayChoice::Custom(url)
    } else {
        RelayChoice::Default
    };
    let use_relay = relay != RelayChoice::Disabled;
    let metrics = Arc::new(CoreMetrics::default());
    let (node, events) = Node::start(NetConfig {
        identity: profile.identity.clone(),
        display_name: profile.display_name.clone(),
        relay,
        metrics,
    })
    .await?;
    println!(
        "{}",
        brand::hud(["PEER", &me.short(), &profile.display_name])
    );
    if use_relay && !node.wait_online(vxl_net::ONLINE_TIMEOUT).await {
        eprintln!("warning: no home relay yet; continuing with direct addresses only");
    }

    match &cli.command {
        Command::Create {
            sector_name,
            key_out,
            no_expiry,
        } => {
            let sector = vxl_net::new_sector(&profile.identity, sector_name)?;
            let rec = SectorRecord {
                sector: sector.clone(),
                cap: None,
                joined_at_ms: now_ms(),
            };
            store.upsert_sector(&rec)?;
            node.join(sector, None, vec![], vec![]).await?;
            let ttl = (!no_expiry).then_some(DEFAULT_INVITE_TTL);
            print_key(&node, &rec, ttl, key_out.as_ref())?;
        }
        Command::Join { key } => {
            let key = SectorKey::decode(key)?;
            key.cap.check(now_ms())?;
            let sector = key.cap.sector();
            if store.sector(&sector.id)?.is_some() {
                bail!("already joined {}; use `vxl up`", sector.name);
            }
            let rec = SectorRecord {
                sector: sector.clone(),
                cap: Some(key.cap.clone()),
                joined_at_ms: now_ms(),
            };
            store.upsert_sector(&rec)?;
            println!("{}", brand::hud(["JOINING", &sector.name]));
            node.join(sector, Some(key.cap), key.bootstrap, vec![])
                .await?;
        }
        Command::Up { key_out } => {
            let recs = store.sectors()?;
            if recs.is_empty() {
                bail!("no stored Sectors; use `create` or `join` first");
            }
            for rec in &recs {
                rejoin(&node, &store, rec).await?;
            }
            if let Some(path) = key_out {
                print_key(&node, &recs[0], Some(DEFAULT_INVITE_TTL), Some(path))?;
            }
        }
        _ => unreachable!("handled above"),
    }

    let deadline = cli.run_for.map(|s| Instant::now() + Duration::from_secs(s));
    pump(&store, events, deadline).await;
    node.shutdown().await;
    Ok(())
}

async fn rejoin(node: &Node, store: &Store, rec: &SectorRecord) -> Result<()> {
    let members = store.members(&rec.sector.id)?;
    let mut boot = Vec::new();
    for m in &members {
        if let Some(a) = store.peer_addr(&m.peer)? {
            boot.push(a);
        }
    }
    if let Some(cap) = &rec.cap
        && let Some(a) = store.peer_addr(&cap.owner)?
        && !boot.iter().any(|b| b.id == a.id)
    {
        boot.push(a);
    }
    let known = members
        .into_iter()
        .map(|m| (m.peer, m.display_name))
        .collect();
    println!("{}", brand::hud(["REJOIN", &rec.sector.name]));
    node.join(rec.sector.clone(), rec.cap.clone(), boot, known)
        .await?;
    Ok(())
}

fn print_key(
    node: &Node,
    rec: &SectorRecord,
    ttl: Option<Duration>,
    out: Option<&PathBuf>,
) -> Result<()> {
    let key = node
        .sector_key(&rec.sector, rec.cap.as_ref(), ttl)?
        .encode()?;
    println!("{}", brand::hud(["SECTOR KEY", &rec.sector.name]));
    println!("{key}");
    if let Some(path) = out {
        std::fs::write(path, &key).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

async fn pump(store: &Store, mut events: mpsc::Receiver<NetEvent>, deadline: Option<Instant>) {
    let mut last_links = Instant::now() - Duration::from_secs(60);
    loop {
        let sleep = async {
            match deadline {
                Some(d) => tokio::time::sleep_until(d.into()).await,
                None => std::future::pending().await,
            }
        };
        let ev = tokio::select! {
            ev = events.recv() => ev,
            _ = tokio::signal::ctrl_c() => None,
            _ = sleep => None,
        };
        let Some(ev) = ev else { break };
        match ev {
            NetEvent::Online { addr } => {
                let relay = addr.relay.as_deref().unwrap_or("--");
                println!(
                    "{}",
                    brand::hud(["ONLINE", &format!("{} direct", addr.ips.len()), relay])
                );
            }
            NetEvent::Roster { sector_id, members } => {
                let list: Vec<String> = members
                    .iter()
                    .map(|m| {
                        format!(
                            "{}{}{}",
                            m.display_name,
                            if m.is_self { " (you)" } else { "" },
                            if m.online { "" } else { " [offline]" }
                        )
                    })
                    .collect();
                println!(
                    "{}",
                    brand::hud(["ROSTER", &sector_id.short(), &list.join(", ")])
                );
            }
            NetEvent::MemberSeen {
                sector_id,
                addr,
                display_name,
                seen_ms,
            } => {
                if let Err(err) = store
                    .upsert_member(&sector_id, &addr.id, &display_name, seen_ms)
                    .and_then(|()| store.upsert_peer_addr(&addr, seen_ms))
                {
                    eprintln!("store error: {err}");
                }
            }
            NetEvent::Links(links) => {
                if last_links.elapsed() < Duration::from_secs(5) || links.is_empty() {
                    continue;
                }
                last_links = Instant::now();
                for l in links {
                    let rtt = l.rtt_ms.map_or("--".into(), |r| format!("{r:.1} ms"));
                    let loss = l.loss_pct.map_or("--".into(), |p| format!("{p:.1}%"));
                    let path = match (l.connected, l.path) {
                        (false, _) => "LINKING",
                        (true, Some(vxl_net::PathKind::Relay)) => "RELAY",
                        (true, Some(vxl_net::PathKind::Direct)) => "DIRECT",
                        (true, None) => "--",
                    };
                    println!(
                        "{}",
                        brand::hud(["LINK", &l.peer.short(), &rtt, &loss, path])
                    );
                }
            }
        }
    }
}
