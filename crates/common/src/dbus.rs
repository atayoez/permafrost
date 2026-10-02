//! The service's D-Bus interface, as seen by clients.
//!
//! Structured values travel as JSON strings so the model can grow without
//! changing the D-Bus signature.

use zbus::proxy;

#[proxy(
    interface = "io.github.atayoez.Permafrost1",
    default_service = "io.github.atayoez.Permafrost1",
    default_path = "/io/github/atayoez/Permafrost1"
)]
pub trait Permafrost {
    /// The current `Status` as JSON.
    fn get_status(&self) -> zbus::Result<String>;

    /// Creates or updates a `BlockList` (JSON). An empty id creates one. Returns the id.
    fn save_list(&self, list: &str) -> zbus::Result<String>;

    fn delete_list(&self, id: &str) -> zbus::Result<()>;

    /// Creates or updates a `Schedule` (JSON). An empty id creates one. Returns the id.
    fn save_schedule(&self, schedule: &str) -> zbus::Result<String>;

    fn delete_schedule(&self, id: &str) -> zbus::Result<()>;

    /// Replaces the `Settings` (JSON).
    fn set_settings(&self, settings: &str) -> zbus::Result<()>;

    /// Resets preset filters and settings; custom filters and schedules stay.
    fn restore_defaults(&self) -> zbus::Result<()>;

    /// `breaks` is a `Breaks` (JSON) for Pomodoro cycles, or empty for none.
    fn start_freeze(&self, lists: &[&str], seconds: u64, locked: bool, breaks: &str) -> zbus::Result<()>;

    fn add_time(&self, seconds: u64) -> zbus::Result<()>;

    fn stop_freeze(&self) -> zbus::Result<()>;

    /// Emitted with the new `Status` (JSON) whenever it changes.
    #[zbus(signal)]
    fn status_changed(&self, status: &str) -> zbus::Result<()>;

    /// Emitted when a blocked app was closed. `until` is Unix seconds.
    #[zbus(signal)]
    fn app_blocked(&self, app_id: &str, until: i64) -> zbus::Result<()>;
}
