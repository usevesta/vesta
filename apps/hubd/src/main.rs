//! `vesta-hubd`: the headless Hub, for Linux boxes without a screen and for
//! the development loop.
//!
//! ```sh
//! cargo run -p vesta-hubd -- --dev                      # simulated home, no sign-in, edits in memory
//! vesta-hubd --config config.json --dashboard dashboard.json --ui web/dist
//! vesta-hubd tuya-import devices.json --out config.json # add Tuya devices (writes their keys!)
//! ```
//!
//! Listens on loopback only (`127.0.0.1:8080` by default). Logs go to
//! stderr; set `RUST_LOG` to change the level (e.g. `RUST_LOG=debug`).
//!
//! Without `--dev` the API needs a session: at start-up the hub logs a
//! one-time sign-in URL (single use, valid for 60 s) to open in a browser
//! on this machine. `--dev` turns sign-in off (development only).

mod tuya_import;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;
use vesta_core::{Dashboard, HubConfig};
use vesta_drivers::Registry;
use vesta_hub::{ApiAuth, Hub, HubOptions, JsonFileStore, LOGIN_CODE_TTL, UiFiles, bind_loopback};

/// The simulated home used by `--dev`. No secrets: every device uses the
/// `sim` driver.
const DEV_CONFIG: &str = vesta_drivers::demo::CONFIG;
const DEV_DASHBOARD: &str = vesta_drivers::demo::DASHBOARD;

#[derive(Debug, Parser)]
#[command(
    name = "vesta-hubd",
    version,
    about = "Headless VESTA hub",
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
struct Args {
    #[command(subcommand)]
    tool: Option<Tool>,

    /// Hub config (`config.json`).
    #[arg(
        long,
        value_name = "PATH",
        required_unless_present = "dev",
        conflicts_with = "dev"
    )]
    config: Option<PathBuf>,

    /// Dashboard (`dashboard.json`). Changes made on screen are saved back
    /// to this file.
    #[arg(
        long,
        value_name = "PATH",
        required_unless_present = "dev",
        conflicts_with = "dev"
    )]
    dashboard: Option<PathBuf>,

    /// Port to listen on; overrides `hub.port` from the config (default 8080).
    #[arg(long)]
    port: Option<u16>,

    /// Run the built-in simulated home (no hardware, no config files) with
    /// sign-in turned off. Development only.
    #[arg(long)]
    dev: bool,

    /// Serve the wall-panel UI from DIR (a `web/dist` build) on the hub's
    /// own address.
    #[arg(long, value_name = "DIR")]
    ui: Option<PathBuf>,

    /// Another page origin that may use a session, exactly, e.g.
    /// `http://localhost:5173` for the Vite dev UI. Repeatable.
    #[arg(long, value_name = "ORIGIN", conflicts_with = "dev")]
    allow_origin: Vec<String>,
}

/// One-off tools; without one, the hub runs.
#[derive(Debug, Subcommand)]
enum Tool {
    /// Add Tuya devices from tinytuya's device list (`devices.json`) to a
    /// hub config. The config then holds each device's local key, which
    /// controls the device: keep it out of git, chats and screenshots.
    TuyaImport(tuya_import::TuyaImport),
}

fn read(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))
}

fn load(args: &Args) -> anyhow::Result<(HubConfig, Dashboard)> {
    let (config, config_source, dashboard, dashboard_source) = match (&args.config, &args.dashboard)
    {
        (Some(config), Some(dashboard)) => (
            read(config)?,
            config.display().to_string(),
            read(dashboard)?,
            dashboard.display().to_string(),
        ),
        _ => (
            DEV_CONFIG.to_owned(),
            "built-in dev config".to_owned(),
            DEV_DASHBOARD.to_owned(),
            "built-in dev dashboard".to_owned(),
        ),
    };
    let config =
        HubConfig::from_json(&config).with_context(|| format!("invalid {config_source}"))?;
    let dashboard =
        Dashboard::from_json(&dashboard).with_context(|| format!("invalid {dashboard_source}"))?;
    Ok((config, dashboard))
}

fn options(args: &Args) -> anyhow::Result<HubOptions> {
    let ui = match &args.ui {
        Some(dir) => {
            let files = UiFiles::load_dir(dir)
                .with_context(|| format!("could not read the UI from {}", dir.display()))?;
            Some(Arc::new(files) as Arc<dyn vesta_hub::UiAssets>)
        }
        None => None,
    };
    Ok(HubOptions {
        // Only `--dev` turns sign-in off.
        auth: if args.dev {
            ApiAuth::Disabled
        } else {
            ApiAuth::Sessions
        },
        allowed_origins: args.allow_origin.clone(),
        ui,
        ..HubOptions::default()
    })
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "cannot listen for Ctrl-C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("shutting down");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    if let Some(Tool::TuyaImport(import)) = &args.tool {
        return tuya_import::run(import).await;
    }

    let (mut config, dashboard) = load(&args)?;
    if let Some(port) = args.port {
        config.hub.port = port;
    }
    if args.dev {
        tracing::info!("running the simulated home (--dev)");
    }
    let addr = SocketAddr::new(config.hub.bind_address, config.hub.port);

    let mut hub = Hub::start(&config, dashboard, &Registry::builtin(), options(&args)?).await?;
    // `--dev` keeps edits in memory: a restart resets the simulated home.
    if let Some(path) = &args.dashboard {
        hub = hub.with_dashboard_store(JsonFileStore::new(path));
    }
    let listener = bind_loopback(addr).await?;
    if let Some(codes) = hub.login_codes() {
        let local = listener.local_addr()?;
        let path = codes.issue()?.login_path();
        tracing::info!(
            "to open the panel in a browser on this machine, go to http://{local}{path} \
             (works once, within {} s; restart the hub for a new one)",
            LOGIN_CODE_TTL.as_secs()
        );
        if args.ui.is_none() {
            tracing::info!(
                "no --ui: with the Vite dev UI, start the hub with \
                 --allow-origin http://localhost:5173 and open the same path there"
            );
        }
    }
    hub.serve(listener, shutdown_signal()).await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_is_consistent() {
        Args::command().debug_assert();
        assert!(Args::try_parse_from(["vesta-hubd", "--dev"]).is_ok());
        assert!(Args::try_parse_from(["vesta-hubd", "--dev", "--port", "9000"]).is_ok());
        assert!(
            Args::try_parse_from(["vesta-hubd", "--config", "c.json", "--dashboard", "d.json"])
                .is_ok()
        );
        assert!(Args::try_parse_from(["vesta-hubd"]).is_err());
        assert!(Args::try_parse_from(["vesta-hubd", "--config", "c.json"]).is_err());
        assert!(Args::try_parse_from(["vesta-hubd", "--dev", "--config", "c.json"]).is_err());
        assert!(
            Args::try_parse_from([
                "vesta-hubd",
                "--dev",
                "--allow-origin",
                "http://localhost:1"
            ])
            .is_err()
        );

        let import = Args::try_parse_from([
            "vesta-hubd",
            "tuya-import",
            "devices.json",
            "--out",
            "config.json",
            "--only",
            "Hall plug",
            "--only",
            "Robot",
            "--read-only",
            "--listen",
            "0",
            "--check",
        ])
        .unwrap();
        let Some(Tool::TuyaImport(import)) = import.tool else {
            panic!("expected tuya-import");
        };
        assert_eq!(import.only, ["Hall plug", "Robot"]);
        assert!(import.read_only && import.check);
        assert_eq!(import.listen, 0);
        assert!(Args::try_parse_from(["vesta-hubd", "tuya-import", "devices.json"]).is_err());
        assert!(
            Args::try_parse_from([
                "vesta-hubd",
                "--dev",
                "tuya-import",
                "devices.json",
                "--out",
                "c.json"
            ])
            .is_err()
        );
    }

    #[test]
    fn sign_in_is_off_only_with_dev() {
        let dev = Args::try_parse_from(["vesta-hubd", "--dev"]).unwrap();
        assert_eq!(options(&dev).unwrap().auth, ApiAuth::Disabled);
        let real = Args::try_parse_from([
            "vesta-hubd",
            "--config",
            "c.json",
            "--dashboard",
            "d.json",
            "--allow-origin",
            "http://localhost:5173",
        ])
        .unwrap();
        let options = options(&real).unwrap();
        assert_eq!(options.auth, ApiAuth::Sessions);
        assert_eq!(options.allowed_origins, ["http://localhost:5173"]);
        assert!(options.ui.is_none());

        let missing_ui = Args::try_parse_from(["vesta-hubd", "--dev", "--ui", "/nonexistent"]);
        assert!(super::options(&missing_ui.unwrap()).is_err());
    }

    #[test]
    fn dev_fixtures_describe_the_simulated_home() {
        let args = Args::try_parse_from(["vesta-hubd", "--dev"]).unwrap();
        let (config, dashboard) = load(&args).unwrap();
        assert!(config.hub.bind_address.is_loopback());
        assert_eq!(config.hub.port, 8080);
        assert!(config.devices.iter().all(|device| device.driver == "sim"));

        // Every target the dashboard uses exists in the simulated home.
        let registry = Registry::builtin();
        let mut targets = BTreeSet::new();
        for device in &config.devices {
            for endpoint in registry.build(device).unwrap().describe().endpoints {
                targets.insert(format!("{}/{}", device.id, endpoint.id));
            }
        }
        let mut used = BTreeSet::new();
        let mut widget_types = BTreeSet::new();
        for page in &dashboard.pages {
            for widget in &page.widgets {
                widget_types.insert(widget.widget_type.as_str());
                if let Some(target) = &widget.target {
                    used.insert(target.to_string());
                }
            }
        }
        for event in &dashboard.kiosk.wake_on {
            used.insert(event.target.to_string());
        }
        for target in &used {
            assert!(targets.contains(target), "{target} is not in the dev home");
        }

        assert_eq!(dashboard.pages.len(), 4);
        assert_eq!(
            widget_types,
            BTreeSet::from([
                "camera-preview",
                "clock-weather",
                "cover-control",
                "door-button",
                "ev-charger",
                "fan-control",
                "light-control",
                "outlet-control",
                "robot-vacuum",
            ])
        );
        assert!(
            dashboard
                .kiosk
                .wake_on
                .iter()
                .any(|event| event.event == vesta_core::EventKind::DoorbellPressed)
        );
    }
}
