use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use permafrost_common::model::Status;
use permafrost_common::presets;

use crate::format;
use crate::ring::CountdownRing;
use crate::window::Window;

/// Presets checked by default on the Freeze page.
const DEFAULT_PRESETS: &[&str] = &["social", "video"];

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/atayoez/Permafrost/ui/freeze-page.ui")]
    pub struct FreezePage {
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub duration_group: TemplateChild<adw::ToggleGroup>,
        #[template_child]
        pub custom_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub hours_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub minutes_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub lists_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub lock_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub freeze_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub ring: TemplateChild<CountdownRing>,
        #[template_child]
        pub countdown_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub until_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub lock_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub active_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub add_time_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub stop_button: TemplateChild<gtk::Button>,

        /// One switch per block list, by list id.
        pub list_rows: RefCell<Vec<(String, adw::SwitchRow)>>,
        /// Lists the person turned off, remembered across rebuilds.
        pub unchecked: RefCell<BTreeSet<String>>,
        /// Lists this page has shown before, to pick a default for new ones.
        pub seen: RefCell<BTreeSet<String>>,
        pub active_rows: RefCell<Vec<adw::ActionRow>>,
        pub started_at: Cell<i64>,
        pub ends_at: Cell<i64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FreezePage {
        const NAME: &'static str = "PermafrostFreezePage";
        type Type = super::FreezePage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            CountdownRing::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for FreezePage {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for FreezePage {}
    impl BinImpl for FreezePage {}
}

glib::wrapper! {
    pub struct FreezePage(ObjectSubclass<imp::FreezePage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FreezePage {
    fn setup(&self) {
        let imp = self.imp();
        let refresh = glib::clone!(
            #[weak(rename_to = page)]
            self,
            move || page.update_freeze_button()
        );
        imp.duration_group.connect_active_name_notify({
            let refresh = refresh.clone();
            move |_| refresh()
        });
        imp.hours_row.connect_value_notify({
            let refresh = refresh.clone();
            move |_| refresh()
        });
        imp.minutes_row.connect_value_notify(move |_| refresh());

        imp.freeze_button.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.start_freeze()
        ));
        imp.add_time_button.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| {
                if let Some(win) = page.window() {
                    win.spawn(|client| async move { client.add_time(15 * 60).await });
                }
            }
        ));
        imp.stop_button.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| {
                if let Some(win) = page.window() {
                    win.spawn(|client| async move { client.stop_freeze().await });
                }
            }
        ));
        self.update_freeze_button();
    }

    fn window(&self) -> Option<Window> {
        self.root().and_downcast()
    }

    fn minutes(&self) -> u32 {
        let imp = self.imp();
        match imp.duration_group.active_name().as_deref() {
            Some("custom") | None => (imp.hours_row.value() as u32) * 60 + imp.minutes_row.value() as u32,
            Some(minutes) => minutes.parse().unwrap_or(60),
        }
    }

    fn selected_lists(&self) -> Vec<String> {
        self.imp().list_rows.borrow().iter().filter(|(_, row)| row.is_active()).map(|(id, _)| id.clone()).collect()
    }

    fn update_freeze_button(&self) {
        let imp = self.imp();
        imp.custom_group.set_visible(imp.duration_group.active_name().as_deref() == Some("custom"));
        let minutes = self.minutes();
        imp.freeze_button.set_label(&format::freeze_button(minutes.max(1)));
        imp.freeze_button.set_sensitive(minutes >= 1 && !self.selected_lists().is_empty());
    }

    fn start_freeze(&self) {
        let imp = self.imp();
        let lists = self.selected_lists();
        let seconds = u64::from(self.minutes()) * 60;
        let locked = imp.lock_row.is_active();
        if let Some(win) = self.window() {
            win.spawn(move |client| async move { client.start_freeze(&lists, seconds, locked).await });
        }
    }

    pub fn set_status(&self, status: &Status) {
        let imp = self.imp();
        self.rebuild_list_rows(status);

        let freeze = status.state.freeze.as_ref().filter(|f| f.ends_at > status.now);
        let Some(freeze) = freeze else {
            imp.stack.set_visible_child_name("idle");
            return;
        };
        imp.started_at.set(freeze.started_at);
        imp.ends_at.set(freeze.ends_at);
        imp.until_label.set_label(&format!("Frozen until {}", format::clock(freeze.ends_at)));
        imp.lock_label.set_label(if freeze.locked {
            "Locked — you can add time, but not stop early"
        } else {
            "Not locked — you can stop anytime"
        });
        imp.stop_button.set_visible(!freeze.locked);

        for row in imp.active_rows.take() {
            imp.active_group.remove(&row);
        }
        let mut rows = Vec::new();
        for list in freeze.lists.iter().filter_map(|id| status.state.list(id)) {
            let row = adw::ActionRow::builder()
                .title(&list.name)
                .subtitle(format::list_summary(list, status))
                .build();
            row.add_prefix(&gtk::Image::from_icon_name("security-high-symbolic"));
            imp.active_group.add(&row);
            rows.push(row);
        }
        imp.active_rows.replace(rows);
        imp.stack.set_visible_child_name("frozen");
        self.tick(status.now);
    }

    fn rebuild_list_rows(&self, status: &Status) {
        let imp = self.imp();
        let same = {
            let rows = imp.list_rows.borrow();
            rows.len() == status.state.lists.len()
                && rows.iter().zip(&status.state.lists).all(|((id, row), list)| *id == list.id && row.title() == list.name)
        };
        if same {
            for ((_, row), list) in imp.list_rows.borrow().iter().zip(&status.state.lists) {
                row.set_subtitle(&format::list_summary(list, status));
            }
            return;
        }

        for (_, row) in imp.list_rows.take() {
            imp.lists_group.remove(&row);
        }
        let mut rows = Vec::new();
        for list in &status.state.lists {
            // Presets start unchecked, apart from the two most people freeze.
            if imp.seen.borrow_mut().insert(list.id.clone())
                && presets::find(&list.id).is_some()
                && !DEFAULT_PRESETS.contains(&list.id.as_str())
            {
                imp.unchecked.borrow_mut().insert(list.id.clone());
            }
            let row = adw::SwitchRow::builder()
                .title(&list.name)
                .subtitle(format::list_summary(list, status))
                .active(!imp.unchecked.borrow().contains(&list.id))
                .build();
            let id = list.id.clone();
            row.connect_active_notify(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |row| {
                    let mut unchecked = page.imp().unchecked.borrow_mut();
                    if row.is_active() {
                        unchecked.remove(&id);
                    } else {
                        unchecked.insert(id.clone());
                    }
                    drop(unchecked);
                    page.update_freeze_button();
                }
            ));
            imp.lists_group.add(&row);
            rows.push((list.id.clone(), row));
        }
        imp.list_rows.replace(rows);
        self.update_freeze_button();
    }

    pub fn tick(&self, now: i64) {
        let imp = self.imp();
        let (start, end) = (imp.started_at.get(), imp.ends_at.get());
        if end <= start {
            return;
        }
        let remaining = (end - now).max(0);
        imp.countdown_label.set_label(&format::countdown(remaining));
        imp.ring.set_fraction(remaining as f64 / (end - start) as f64);
    }
}
