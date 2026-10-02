//! The Settings window.

use adw::prelude::*;
use gtk::glib;
use permafrost_common::model::{Settings, Status};

use crate::window::Window;

pub fn present(win: &Window, status: &Status) {
    let settings = status.state.settings.clone();
    let locked = !status.locked_lists.is_empty();

    let dialog = adw::PreferencesDialog::builder().title("Settings").search_enabled(false).build();
    let page = adw::PreferencesPage::new();

    let freezing = adw::PreferencesGroup::builder().title("Freezing").build();
    let lock = adw::SwitchRow::builder()
        .title("Lock Until the Timer Ends")
        .subtitle("New freezes can’t be stopped early, only extended")
        .active(settings.lock_freezes)
        .build();
    freezing.add(&lock);
    page.add(&freezing);

    let search = adw::PreferencesGroup::builder()
        .title("Search")
        .description("Always on, not only during a freeze. While something is frozen, it can be turned on but not off.")
        .build();
    let safe_search = adw::SwitchRow::builder()
        .title("Force SafeSearch")
        .subtitle("Filters explicit results on Google, Bing, DuckDuckGo and YouTube")
        .active(settings.safe_search)
        .sensitive(!(locked && settings.safe_search))
        .build();
    search.add(&safe_search);
    page.add(&search);

    let reset = adw::PreferencesGroup::builder()
        .description(if locked {
            "Available once nothing is frozen"
        } else {
            "Resets the built-in filters and these settings. Your own filters and schedules stay."
        })
        .build();
    let restore = adw::ButtonRow::builder()
        .title("_Restore Defaults…")
        .use_underline(true)
        .sensitive(!locked)
        .css_classes(["destructive-action"])
        .build();
    reset.add(&restore);
    page.add(&reset);

    // Each switch saves straight away, like other GNOME settings.
    let save = glib::clone!(
        #[weak]
        win,
        #[weak]
        lock,
        #[weak]
        safe_search,
        move || {
            let settings = Settings {
                lock_freezes: lock.is_active(),
                safe_search: safe_search.is_active(),
                ..settings.clone()
            };
            win.spawn(move |client| async move { client.set_settings(&settings).await });
        }
    );
    lock.connect_active_notify({
        let save = save.clone();
        move |_| save()
    });
    safe_search.connect_active_notify(move |_| save());

    restore.connect_activated(glib::clone!(
        #[weak]
        win,
        #[weak]
        dialog,
        move |_| confirm_restore(&win, &dialog)
    ));

    dialog.add(&page);
    dialog.present(Some(win));
}

fn confirm_restore(win: &Window, settings: &adw::PreferencesDialog) {
    let confirm = adw::AlertDialog::builder()
        .heading("Restore Defaults?")
        .body("The built-in filters go back to their original sites and apps, deleted ones come back, and settings are reset. Your own filters and schedules stay.")
        .default_response("cancel")
        .close_response("cancel")
        .build();
    confirm.add_responses(&[("cancel", "_Cancel"), ("restore", "_Restore")]);
    confirm.set_response_appearance("restore", adw::ResponseAppearance::Destructive);
    confirm.connect_response(
        Some("restore"),
        glib::clone!(
            #[weak]
            win,
            #[weak]
            settings,
            move |_, _| {
                win.spawn(|client| async move { client.restore_defaults().await });
                settings.close();
                win.toast("Defaults restored");
            }
        ),
    );
    confirm.present(Some(settings));
}
