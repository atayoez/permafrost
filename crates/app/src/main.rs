mod app_picker;
mod application;
mod background;
mod client;
mod format;
mod freeze_page;
mod list_page;
mod ring;
mod schedule_dialog;
mod schedules_page;
mod window;

use gtk::{gio, glib};

fn main() -> glib::ExitCode {
    gio::resources_register_include!("permafrost.gresource").expect("bundled resources load");
    application::run()
}
