//! `vxl`: headless VEXEL peer.
//!
//! Phase 0 ships `info` and `mem`. Later phases add `sector create|join`, `voice tone`, `aux play` and more,
//! so P2P flows can be tested with several peers on one machine.

use anyhow::Result;
use clap::{Parser, Subcommand};
use vxl_core::brand;

#[derive(Debug, Parser)]
#[command(name = "vxl", version, about = brand::OS_TAG)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print build and brand info.
    Info,
    /// Print this process's working set (sanity check for the Zero-Drag RAM probe).
    Mem,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Info => {
            println!("{}", brand::OS_TAG);
            println!("{}", brand::TAGLINE);
            println!("{}", brand::hud(["VERSION", brand::VERSION]));
            println!(
                "{}",
                brand::hud(["SCHEME", &format!("{}://", brand::URL_SCHEME)])
            );
        }
        Command::Mem => {
            let mb = vxl_platform::memory::working_set_bytes().map_or_else(
                || "--".to_owned(),
                |b| format!("{:.1} MB", b as f64 / 1_048_576.0),
            );
            println!("{}", brand::hud(["CORE", &mb]));
        }
    }
    Ok(())
}
