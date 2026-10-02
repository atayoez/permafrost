//! The D-Bus interface clients use. See `permafrost_common::dbus` for the client side.

use std::sync::{Arc, Mutex, MutexGuard};

use zbus::fdo;
use zbus::interface;
use zbus::object_server::SignalEmitter;

use crate::daemon::{Daemon, Error};

pub struct Service {
    pub daemon: Arc<Mutex<Daemon>>,
}

impl From<Error> for fdo::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::Denied(msg) => fdo::Error::AccessDenied(msg),
            Error::Invalid(msg) => fdo::Error::InvalidArgs(msg),
            Error::Failed(e) => fdo::Error::Failed(format!("{e:#}")),
        }
    }
}

impl Service {
    fn daemon(&self) -> MutexGuard<'_, Daemon> {
        self.daemon.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs a change, then tells clients about the new status.
    async fn change<T>(
        &self,
        emitter: &SignalEmitter<'_>,
        f: impl FnOnce(&mut Daemon) -> crate::daemon::Result<T>,
    ) -> fdo::Result<T> {
        let (value, status) = {
            let mut daemon = self.daemon();
            let value = f(&mut daemon)?;
            (value, daemon.status_json())
        };
        Self::status_changed(emitter, &status).await?;
        Ok(value)
    }
}

#[interface(name = "io.github.atayoez.Permafrost1")]
impl Service {
    fn get_status(&self) -> String {
        self.daemon().status_json()
    }

    async fn save_list(&self, list: &str, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<String> {
        self.change(&emitter, |d| d.save_list(list)).await
    }

    async fn delete_list(&self, id: &str, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.change(&emitter, |d| d.delete_list(id)).await
    }

    async fn save_schedule(
        &self,
        schedule: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<String> {
        self.change(&emitter, |d| d.save_schedule(schedule)).await
    }

    async fn delete_schedule(&self, id: &str, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.change(&emitter, |d| d.delete_schedule(id)).await
    }

    async fn set_filters(&self, filters: &str, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.change(&emitter, |d| d.set_filters(filters)).await
    }

    async fn start_freeze(
        &self,
        lists: Vec<String>,
        seconds: u64,
        locked: bool,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.change(&emitter, |d| d.start_freeze(lists, seconds, locked)).await
    }

    async fn add_time(&self, seconds: u64, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.change(&emitter, |d| d.add_time(seconds)).await
    }

    async fn stop_freeze(&self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.change(&emitter, |d| d.stop_freeze()).await
    }

    #[zbus(signal)]
    pub async fn status_changed(emitter: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn app_blocked(emitter: &SignalEmitter<'_>, app_id: &str, until: i64) -> zbus::Result<()>;
}
