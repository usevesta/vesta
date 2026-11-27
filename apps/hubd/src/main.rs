//! `vesta-hubd`: the headless Hub, for Linux boxes without a screen and for
//! the development loop.
//!
//! ```sh
//! cargo run -p vesta-hubd -- --dev                      # simulated home
//! vesta-hubd --config config.json --dashboard dashboard.json
//! ```
//!
//! Listens on loopback only (`127.0.0.1:8080` by default). Logs go to
//! stderr; set `RUST_LOG` to change the level (e.g. `RUST_LOG=debug`).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Parser;
use tracing_subscriber::EnvFilter;
use vesta_core::{Dashboard, HubConfig};
use vesta_drivers::Registry;
use vesta_hub::{Hub, HubOptions, bind_loopback};

/// The simulated home used by `--dev`. No secrets: every device uses the
/// `sim` driver.
const DEV_CONFIG: &str = include_str!("../fixtures/dev-config.json");
const DEV_DASHBOARD: &str = include_str!("../fixtures/dev-dashboard.json");

#[derive(Debug, Parser)]
#[command(name = "vesta-hubd", version, about = "Headless VESTA hub")]
struct Args {
    /// Hub config (`config.json`).
    #[arg(
        long,
        value_name = "PATH",
        required_unless_present = "dev",
        conflicts_with = "dev"
    )]
    config: Option<PathBuf>,

    /// Dashboard (`dashboard.json`).
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

    /// Run the built-in simulated home (no hardware, no config files).
    #[arg(long)]
    dev: bool,
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

    let (mut config, dashboard) = load(&args)?;
    if let Some(port) = args.port {
        config.hub.port = port;
    }
    if args.dev {
        tracing::info!("running the simulated home (--dev)");
    }
    let addr = SocketAddr::new(config.hub.bind_address, config.hub.port);

    let hub = Hub::start(
        &config,
        dashboard,
        &Registry::builtin(),
        HubOptions::default(),
    )
    .await?;
    let listener = bind_loopback(addr).await?;
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

        assert_eq!(dashboard.pages.len(), 2);
        assert_eq!(
            widget_types,
            BTreeSet::from([
                "camera-preview",
                "clock-weather",
                "cover-control",
                "door-button",
                "fan-control",
                "light-control",
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
