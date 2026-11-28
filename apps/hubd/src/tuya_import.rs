//! `vesta-hubd tuya-import`: adds Tuya devices from tinytuya's device list
//! to a hub config, so nobody edits JSON by hand.
//!
//! ```sh
//! vesta-hubd tuya-import devices.json --out config.json [--snapshot snapshot.json] \
//!     [--only "Hall plug" --only "Robot"] [--read-only] [--listen 12]
//! ```
//!
//! It listens for the devices' own broadcasts for a few seconds (nothing
//! is sent to them) to learn their protocol version and address. The
//! config it writes holds every device's local key, so it is written
//! readable by its owner only, and nothing secret is printed.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use vesta_drivers::tuya::{self, Discovery, Hint, Options, SkipReason, TuyaConfig};

#[derive(Debug, clap::Args)]
pub struct TuyaImport {
    /// tinytuya's `devices.json` (from `python -m tinytuya wizard`).
    pub devices: PathBuf,
    /// The hub config to write. If it exists, the devices are added to it
    /// and everything else is kept.
    #[arg(long, value_name = "PATH")]
    pub out: PathBuf,
    /// tinytuya's `snapshot.json` (from `python -m tinytuya scan`): where
    /// devices were found, for those that do not announce themselves.
    #[arg(long, value_name = "PATH")]
    pub snapshot: Option<PathBuf>,
    /// Only the device with this name (as in the Smart Life app).
    /// Repeatable.
    #[arg(long = "only", value_name = "NAME")]
    pub only: Vec<String>,
    /// Watch only: VESTA shows these devices but never sends them a
    /// command.
    #[arg(long)]
    pub read_only: bool,
    /// Seconds to listen for the devices on the network (0: do not).
    #[arg(long, value_name = "SECONDS", default_value_t = 12)]
    pub listen: u64,
    /// Afterwards, read each device's status once to check that its key
    /// works. Only asks; never changes anything on the devices.
    #[arg(long)]
    pub check: bool,
}

fn read(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))
}

/// Writes `text` to `path` atomically, readable and writable by its owner
/// only (it holds device keys).
fn write_private(path: &Path, text: &str) -> anyhow::Result<()> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("the output path has no file name")?;
    let temporary = dir.join(format!(".{name}.tmp"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("could not write {}", temporary.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(())
}

pub async fn run(args: &TuyaImport) -> anyhow::Result<()> {
    let devices = read(&args.devices)?;
    let current = if args.out.exists() {
        Some(read(&args.out)?)
    } else {
        None
    };
    let existing = tuya::existing(current.as_deref()).with_context(|| {
        format!(
            "{} is not a valid hub config; pick another --out",
            args.out.display()
        )
    })?;
    let snapshot = match &args.snapshot {
        Some(path) => tuya::snapshot_hints(&read(path)?)?,
        None => HashMap::new(),
    };

    let mut heard = HashMap::new();
    if args.listen > 0 {
        eprintln!(
            "Listening for devices on the network for {} s…",
            args.listen
        );
        let discovery = Discovery::shared();
        tokio::time::sleep(Duration::from_secs(args.listen)).await;
        for (id, seen) in discovery.all() {
            heard.insert(
                id,
                Hint {
                    ip: Some(seen.ip),
                    version: seen.version,
                },
            );
        }
    }

    let options = Options {
        read_only: args.read_only,
        only: args.only.clone(),
    };
    let imported = tuya::import(&devices, &heard, &snapshot, &existing, &options)?;
    for wanted in &args.only {
        let known = imported
            .entries
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(wanted.trim()))
            || imported.skipped.iter().any(|skipped| {
                skipped.reason != SkipReason::NotSelected
                    && skipped.name.eq_ignore_ascii_case(wanted.trim())
            });
        if !known {
            eprintln!("  ? no device is called “{wanted}” in the list");
        }
    }
    for entry in &imported.entries {
        let place = if entry.seen {
            "found on the network"
        } else if entry.has_address {
            "address from an earlier scan"
        } else {
            "no address yet: VESTA waits for it to announce itself"
        };
        println!(
            "  + {} → {} (protocol {}, {place})",
            entry.name, entry.id, entry.version
        );
    }
    for skipped in &imported.skipped {
        let why = match &skipped.reason {
            SkipReason::NotSelected => continue,
            SkipReason::Unsupported { category } => {
                format!("not supported yet (Tuya category “{category}”)")
            }
            SkipReason::VersionUnknown => {
                "not found on the network (is it on? try --snapshot or a longer --listen)"
                    .to_owned()
            }
            SkipReason::NoKey => "no local key in the list".to_owned(),
        };
        println!("  - {}: {why}", skipped.name);
    }
    if imported.entries.is_empty() {
        println!("Nothing to add; {} is unchanged.", args.out.display());
        return Ok(());
    }
    let text = tuya::merge(current.as_deref(), &imported.entries)?;
    write_private(&args.out, &text)?;
    println!(
        "Wrote {} ({} Tuya device(s){}).",
        args.out.display(),
        imported.entries.len(),
        if args.read_only { ", watch only" } else { "" }
    );
    println!(
        "It holds the devices' local keys, which control them: keep it out of git, chats and screenshots."
    );
    if args.check {
        check(&imported.entries).await;
    }
    Ok(())
}

/// Reads each device's status once (a query; nothing is set) and says
/// whether it answered.
async fn check(entries: &[tuya::Entry]) {
    let discovery = Discovery::shared();
    for entry in entries {
        let config = match TuyaConfig::from_value(&entry.json["config"]) {
            Ok(config) => config,
            Err(error) => {
                println!("  ✗ {}: {error}", entry.name);
                continue;
            }
        };
        match tuya::read_status(
            &config,
            std::sync::Arc::clone(&discovery),
            Duration::from_secs(10),
        )
        .await
        {
            Ok(status) => println!(
                "  ✓ {} answers (protocol {}, {} values)",
                entry.name,
                status.version,
                status.values.len()
            ),
            Err(error) => println!("  ✗ {} does not answer: {error}", entry.name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vesta-hubd-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn imports_into_a_private_config() {
        let dir = scratch("import");
        let devices = dir.join("devices.json");
        std::fs::write(
            &devices,
            r#"[{ "name": "Hall plug", "id": "bf00000000000000000001", "key": "0123456789abcdef",
                 "category": "cz", "version": "3.4", "ip": "192.0.2.61",
                 "mapping": { "1": { "code": "switch_1", "type": "Boolean", "values": {} } } },
               { "name": "Door", "id": "bf00000000000000000002", "key": "0123456789abcdef",
                 "category": "mcs", "mapping": {} }]"#,
        )
        .unwrap();
        let out = dir.join("config.json");
        let _ = std::fs::remove_file(&out);
        let args = TuyaImport {
            devices,
            out: out.clone(),
            snapshot: None,
            only: Vec::new(),
            read_only: true,
            listen: 0,
            check: false,
        };
        run(&args).await.unwrap();
        let written = std::fs::read_to_string(&out).unwrap();
        let config = vesta_core::HubConfig::from_json(&written).unwrap();
        assert_eq!(config.devices.len(), 1);
        assert_eq!(config.devices[0].id, "hall_plug");
        assert_eq!(
            config.devices[0].config["read_only"],
            serde_json::json!(true)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&out).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // Again: the same device is updated, not added twice.
        run(&args).await.unwrap();
        let again =
            vesta_core::HubConfig::from_json(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(again.devices.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
