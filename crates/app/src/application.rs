use adw::prelude::*;
use gtk::{gio, glib};
use permafrost_common::APP_ID;

use crate::window::Window;

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .resource_base_path("/io/github/atayoez/Permafrost")
        .build();

    app.connect_startup(|app| {
        gtk::Window::set_default_icon_name(APP_ID);
        setup_actions(app);
        app.set_accels_for_action("app.quit", &["<Control>q"]);
        app.set_accels_for_action("app.shortcuts", &["<Control>question"]);
        app.set_accels_for_action("win.new-list", &["<Control>n"]);
        app.set_accels_for_action("window.close", &["<Control>w"]);
    });

    app.connect_activate(|app| {
        let window = app.active_window().and_downcast::<Window>().unwrap_or_else(|| Window::new(app));
        window.present();
    });

    app.run()
}

fn setup_actions(app: &adw::Application) {
    let quit = gio::ActionEntry::builder("quit").activate(|app: &adw::Application, _, _| app.quit()).build();

    let about = gio::ActionEntry::builder("about")
        .activate(|app: &adw::Application, _, _| {
            let dialog = adw::AboutDialog::builder()
                .application_name("Permafrost")
                .application_icon(APP_ID)
                .developer_name("Atay Özcan")
                .developers(["Atay Özcan"])
                .version(env!("CARGO_PKG_VERSION"))
                .website("https://github.com/atayoez/permafrost")
                .issue_url("https://github.com/atayoez/permafrost/issues")
                .license_type(gtk::License::Gpl30)
                .copyright("© 2026 Atay Özcan")
                .build();
            dialog.present(app.active_window().as_ref());
        })
        .build();

    let shortcuts = gio::ActionEntry::builder("shortcuts")
        .activate(|app: &adw::Application, _, _| {
            let section = adw::ShortcutsSection::new(None);
            for (title, accel) in [
                ("New Block List", "<Control>n"),
                ("Keyboard Shortcuts", "<Control>question"),
                ("Close Window", "<Control>w"),
                ("Quit", "<Control>q"),
            ] {
                section.add(adw::ShortcutsItem::new(title, accel));
            }
            let dialog = adw::ShortcutsDialog::new();
            dialog.add(section);
            dialog.present(app.active_window().as_ref());
        })
        .build();

    app.add_action_entries([quit, about, shortcuts]);
}
