//! permafrostd: keeps Permafrost's blocks in place, even when the app is closed.

mod apps;
mod daemon;
mod hosts;
mod safesearch;
mod service;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, bail};
use permafrost_common::{BUS_NAME, OBJECT_PATH};
use tokio::signal::unix::{SignalKind, signal};

use crate::daemon::{Daemon, Options};
use crate::service::Service;

const USAGE: &str = "\
Usage: permafrostd [--session] [--dry-run] [--state-dir DIR] [--hosts FILE]

  --session        Serve on the session bus (development)
  --dry-run        Log what would be blocked instead of blocking it
  --state-dir DIR  Where to keep state (default /var/lib/permafrost)
  --hosts FILE     Hosts file to manage (default /etc/hosts)";

struct Args {
    session: bool,
    options: Options,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut session = false;
    let mut dry_run = false;
    let mut state_dir = PathBuf::from("/var/lib/permafrost");
    let mut hosts_file = PathBuf::from("/etc/hosts");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--session" => session = true,
            "--dry-run" => dry_run = true,
            "--state-dir" => state_dir = args.next().context("--state-dir needs a directory")?.into(),
            "--hosts" => hosts_file = args.next().context("--hosts needs a file")?.into(),
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other}\n\n{USAGE}"),
        }
    }
    Ok(Args { session, options: Options { state_file: state_dir.join("state.json"), hosts_file, dry_run } })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,zbus=warn".into()),
        )
        .without_time()
        .init();

    let args = parse_args()?;
    let daemon = Arc::new(Mutex::new(Daemon::load(args.options)?));

    let builder =
        if args.session { zbus::connection::Builder::session()? } else { zbus::connection::Builder::system()? };
    let connection = builder
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, Service { daemon: daemon.clone() })?
        .build()
        .await
        .context("connecting to D-Bus")?;
    let iface = connection.object_server().interface::<_, Service>(OBJECT_PATH).await?;
    tracing::info!("serving {BUS_NAME} on the {} bus", if args.session { "session" } else { "system" });

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut ticker = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let report = daemon.lock().unwrap_or_else(|p| p.into_inner()).tick();
                let emitter = iface.signal_emitter();
                if let Some(status) = report.status {
                    Service::status_changed(emitter, &status).await?;
                }
                for app in report.closed_apps {
                    Service::app_blocked(emitter, &app, report.locked_until).await?;
                }
            }
            _ = sigterm.recv() => break,
            _ = sigint.recv() => break,
        }
    }

    daemon.lock().unwrap_or_else(|p| p.into_inner()).shutdown();
    tracing::info!("stopped");
    Ok(())
}
