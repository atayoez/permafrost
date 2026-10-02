//! The Settings page.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use permafrost_common::model::{Settings, Status};

use crate::window::Window;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct SettingsPage {
        pub lock: adw::SwitchRow,
        pub safe_search: adw::SwitchRow,
        pub reset_group: adw::PreferencesGroup,
        pub restore: adw::ButtonRow,
        pub settings: RefCell<Settings>,
        /// Set while the page fills in its widgets, so that isn't saved back.
        pub loading: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SettingsPage {
        const NAME: &'static str = "PermafrostSettingsPage";
        type Type = super::SettingsPage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for SettingsPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for SettingsPage {}
    impl BinImpl for SettingsPage {}
}

glib::wrapper! {
    pub struct SettingsPage(ObjectSubclass<imp::SettingsPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SettingsPage {
    fn setup(&self) {
        let imp = self.imp();
        let page = adw::PreferencesPage::new();

        let freezing = adw::PreferencesGroup::builder().title("Freezing").build();
        imp.lock.set_title("Lock Until the Timer Ends");
        imp.lock.set_subtitle("New freezes can’t be stopped early, only extended");
        freezing.add(&imp.lock);
        page.add(&freezing);

        let search = adw::PreferencesGroup::builder()
            .title("Search")
            .description("Always on, not only during a freeze. While something is frozen, it can be turned on but not off.")
            .build();
        imp.safe_search.set_title("Force SafeSearch");
        imp.safe_search.set_subtitle("Filters explicit results on Google, Bing, DuckDuckGo and YouTube");
        search.add(&imp.safe_search);
        page.add(&search);

        imp.restore.set_title("_Restore Defaults…");
        imp.restore.set_use_underline(true);
        imp.restore.add_css_class("destructive-action");
        imp.reset_group.add(&imp.restore);
        page.add(&imp.reset_group);
        self.set_child(Some(&page));

        // Each switch saves straight away, like other GNOME settings.
        for row in [&imp.lock, &imp.safe_search] {
            row.connect_active_notify(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |_| page.save()
            ));
        }
        imp.restore.connect_activated(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.confirm_restore()
        ));
    }

    fn window(&self) -> Option<Window> {
        self.root().and_downcast()
    }

    fn save(&self) {
        let imp = self.imp();
        if imp.loading.get() {
            return;
        }
        let settings = Settings {
            lock_freezes: imp.lock.is_active(),
            safe_search: imp.safe_search.is_active(),
            ..imp.settings.borrow().clone()
        };
        if let Some(win) = self.window() {
            win.spawn(move |client| async move { client.set_settings(&settings).await });
        }
    }

    pub fn set_status(&self, status: &Status) {
        let imp = self.imp();
        let settings = &status.state.settings;
        let locked = !status.locked_lists.is_empty();
        imp.loading.set(true);
        imp.settings.replace(settings.clone());
        imp.lock.set_active(settings.lock_freezes);
        imp.safe_search.set_active(settings.safe_search);
        imp.safe_search.set_sensitive(!(locked && settings.safe_search));
        imp.restore.set_sensitive(!locked);
        imp.reset_group.set_description(Some(if locked {
            "Restoring defaults is available once nothing is frozen"
        } else {
            "Resets the built-in filters and these settings. Your own filters and schedules stay."
        }));
        imp.loading.set(false);
    }

    fn confirm_restore(&self) {
        let Some(win) = self.window() else { return };
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
                move |_, _| {
                    let weak = win.downgrade();
                    win.spawn(move |client| async move {
                        client.restore_defaults().await?;
                        if let Some(win) = weak.upgrade() {
                            win.toast("Defaults restored");
                        }
                        Ok(())
                    });
                }
            ),
        );
        confirm.present(Some(&win));
    }
}
