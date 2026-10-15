//! Waybar widget: prints one JSON line and exits 0, whatever happens.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;

use claude_usagebar::app::{self, Overrides, Settings};
use claude_usagebar::cache::DEFAULT_TTL;
use claude_usagebar::config;
use claude_usagebar::pango::strip_markup;
use claude_usagebar::theme::ColorOverrides;
use claude_usagebar::waybar::{self, WaybarOutput};

#[derive(Debug, Parser)]
#[command(version, about = "Uso do Claude Code para o Waybar")]
struct Cli {
    /// Texto da barra, com {placeholders} (ex.: "{session_pct}% · {session_reset}").
    #[arg(long)]
    format: Option<String>,
    /// Tooltip customizado (padrão: caixa com todas as janelas).
    #[arg(long)]
    tooltip_format: Option<String>,
    /// Ícone antes do texto ("" desliga).
    #[arg(long)]
    icon: Option<String>,
    /// Faixa de tolerância (pontos %) do indicador de ritmo.
    #[arg(long)]
    pace_tolerance: Option<u32>,
    /// Colore os placeholders {*_pace*} pelo ritmo.
    #[arg(long)]
    format_pace_color: bool,
    /// Tooltip com delta em pontos e marcador de tempo decorrido.
    #[arg(long)]
    tooltip_pace_pts: bool,
    #[arg(long)]
    color_low: Option<String>,
    #[arg(long)]
    color_mid: Option<String>,
    #[arg(long)]
    color_high: Option<String>,
    #[arg(long)]
    color_critical: Option<String>,
    /// Não usar as cores do wallbash (HyDE).
    #[arg(long)]
    no_wallbash: bool,
    /// Arquivo de credenciais (padrão: ~/.claude/.credentials.json).
    #[arg(long, env = "CLAUDE_USAGEBAR_CREDS")]
    creds_path: Option<PathBuf>,
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    /// Config TOML (padrão: ~/.config/claude-usagebar/config.toml).
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    user_agent: Option<String>,
    /// Ignora o TTL do cache e avisa o Waybar para redesenhar.
    #[arg(long)]
    refresh: bool,
    /// Força JSON mesmo num terminal.
    #[arg(long, conflicts_with = "pretty")]
    json: bool,
    /// Texto legível em vez de JSON (padrão num terminal).
    #[arg(long)]
    pretty: bool,
    /// Modo contínuo: imprime uma linha a cada N segundos.
    #[arg(long, value_name = "SECS")]
    watch: Option<u64>,
}

impl Cli {
    fn overrides(&self) -> Overrides {
        Overrides {
            format: self.format.clone(),
            tooltip_format: self.tooltip_format.clone(),
            icon: self.icon.clone(),
            pace_tolerance: self.pace_tolerance,
            format_pace_color: self.format_pace_color,
            tooltip_pace_pts: self.tooltip_pace_pts,
            creds_path: self.creds_path.clone(),
            cache_dir: self.cache_dir.clone(),
            user_agent: self.user_agent.clone(),
            no_wallbash: self.no_wallbash,
            colors: ColorOverrides {
                low: self.color_low.clone(),
                mid: self.color_mid.clone(),
                high: self.color_high.clone(),
                critical: self.color_critical.clone(),
            },
        }
    }

    fn json_output(&self) -> bool {
        self.json || (!self.pretty && !std::io::stdout().is_terminal())
    }
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json_output();

    let settings = config::load(cli.config.as_deref())
        .and_then(|cfg| Settings::resolve(&cfg, cli.overrides()));
    let settings = match settings {
        Ok(s) => s,
        Err(e) => {
            emit(&WaybarOutput::error(&app::error_message(&e), None), json);
            return;
        }
    };

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            emit(&WaybarOutput::error(&format!("tokio: {e}"), None), json);
            return;
        }
    };

    let ttl = if cli.refresh { Duration::ZERO } else { DEFAULT_TTL };
    match cli.watch {
        Some(secs) => rt.block_on(async {
            let period = Duration::from_secs(secs.max(5));
            loop {
                emit(&app::waybar_output(&settings, ttl).await, json);
                tokio::time::sleep(period).await;
            }
        }),
        None => {
            let out = rt.block_on(app::waybar_output(&settings, ttl));
            emit(&out, json);
            if cli.refresh {
                waybar::request_refresh();
            }
        }
    }
}

fn emit(out: &WaybarOutput, json: bool) {
    let mut stdout = std::io::stdout().lock();
    let _ = if json {
        stdout.write_all(out.to_json_line().as_bytes())
    } else {
        writeln!(
            stdout,
            "{}  [{}]\n{}",
            strip_markup(&out.text),
            out.class.join(" "),
            strip_markup(&out.tooltip)
        )
    };
    let _ = stdout.flush();
}
