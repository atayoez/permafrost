//! Talking to permafrostd.

use permafrost_common::dbus::PermafrostProxy;
use permafrost_common::model::{BlockList, Schedule, Status};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug)]
pub struct Client {
    proxy: PermafrostProxy<'static>,
}

/// The message to show for a failed call: the service's own words where it gave some.
fn message(e: zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(_, Some(msg), _) => msg,
        zbus::Error::FDO(e) => match *e {
            zbus::fdo::Error::AccessDenied(msg) | zbus::fdo::Error::InvalidArgs(msg) | zbus::fdo::Error::Failed(msg) => msg,
            other => other.to_string(),
        },
        other => other.to_string(),
    }
}

impl Client {
    pub async fn connect() -> Result<Self> {
        let session = std::env::var(permafrost_common::BUS_ENV).is_ok_and(|v| v == "session");
        let connection =
            if session { zbus::Connection::session().await } else { zbus::Connection::system().await }.map_err(message)?;
        let proxy = PermafrostProxy::new(&connection).await.map_err(message)?;
        // Fail early if the service isn't there, rather than on the first click.
        proxy.get_status().await.map_err(message)?;
        Ok(Self { proxy })
    }

    pub fn proxy(&self) -> &PermafrostProxy<'static> {
        &self.proxy
    }

    pub async fn status(&self) -> Result<Status> {
        let json = self.proxy.get_status().await.map_err(message)?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }

    pub async fn save_list(&self, list: &BlockList) -> Result<String> {
        let json = serde_json::to_string(list).map_err(|e| e.to_string())?;
        self.proxy.save_list(&json).await.map_err(message)
    }

    pub async fn delete_list(&self, id: &str) -> Result<()> {
        self.proxy.delete_list(id).await.map_err(message)
    }

    pub async fn save_schedule(&self, schedule: &Schedule) -> Result<String> {
        let json = serde_json::to_string(schedule).map_err(|e| e.to_string())?;
        self.proxy.save_schedule(&json).await.map_err(message)
    }

    pub async fn delete_schedule(&self, id: &str) -> Result<()> {
        self.proxy.delete_schedule(id).await.map_err(message)
    }

    pub async fn start_freeze(&self, lists: &[String], seconds: u64, locked: bool) -> Result<()> {
        let lists: Vec<&str> = lists.iter().map(String::as_str).collect();
        self.proxy.start_freeze(&lists, seconds, locked).await.map_err(message)
    }

    pub async fn add_time(&self, seconds: u64) -> Result<()> {
        self.proxy.add_time(seconds).await.map_err(message)
    }

    pub async fn stop_freeze(&self) -> Result<()> {
        self.proxy.stop_freeze().await.map_err(message)
    }
}
