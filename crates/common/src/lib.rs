//! Types shared by the Permafrost app and its system service.

pub mod dbus;
pub mod domain;
pub mod model;
pub mod presets;

pub const APP_ID: &str = "io.github.atayoez.Permafrost";
pub const BUS_NAME: &str = "io.github.atayoez.Permafrost1";
pub const OBJECT_PATH: &str = "/io/github/atayoez/Permafrost1";

/// Set to `session` to talk to a development daemon on the session bus.
pub const BUS_ENV: &str = "PERMAFROST_BUS";
