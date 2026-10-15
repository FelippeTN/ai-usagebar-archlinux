//! Floating dashboard, opened from the Waybar module's `on-click`.

use std::path::PathBuf;

use clap::Parser;

use claude_usagebar::app::{self, Overrides, Settings};
use claude_usagebar::config;

#[derive(Debug, Parser)]
#[command(version, about = "Painel de uso do Claude Code")]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, env = "CLAUDE_USAGEBAR_CREDS")]
    creds_path: Option<PathBuf>,
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    #[arg(long)]
    no_wallbash: bool,
}

fn main() {
    let cli = Cli::parse();
    let overrides = Overrides {
        creds_path: cli.creds_path,
        cache_dir: cli.cache_dir,
        no_wallbash: cli.no_wallbash,
        ..Default::default()
    };
    let result = config::load(cli.config.as_deref())
        .and_then(|cfg| Settings::resolve(&cfg, overrides))
        .and_then(claude_usagebar::tui::run);
    if let Err(e) = result {
        eprintln!("claude-usagebar-tui: {}", app::error_message(&e));
        std::process::exit(1);
    }
}
