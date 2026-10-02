use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use permafrost_common::model::{Breaks, Freeze, Phase, Status};
use permafrost_common::presets;

use crate::format;
use crate::ring::CountdownRing;
use crate::window::Window;

/// The choices in the Breaks row, in order.
const BREAKS: [Option<Breaks>; 4] = [
    None,
    Some(Breaks { work_minutes: 25, break_minutes: 5 }),
    Some(Breaks { work_minutes: 50, break_minutes: 10 }),
    Some(Breaks { work_minutes: 90, break_minutes: 15 }),
];

/// A filter on the Freeze page: a row to preview it and a switch to block it.
pub struct ListRow {
    id: String,
    row: adw::ActionRow,
    switch: gtk::Switch,
}

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
        pub hours_spin: TemplateChild<gtk::SpinButton>,
        #[template_child]
        pub minutes_spin: TemplateChild<gtk::SpinButton>,
        #[template_child]
        pub lists_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub freeze_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub lock_hint: TemplateChild<gtk::Label>,
        #[template_child]
        pub breaks_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub phase_label: TemplateChild<gtk::Label>,
        /// The current freeze, for showing its break phase every second.
        pub freeze: RefCell<Option<Freeze>>,
        /// The lock setting new freezes use, from Settings.
        pub lock_freezes: Cell<bool>,
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
        pub list_rows: RefCell<Vec<ListRow>>,
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
        imp.hours_spin.connect_value_changed({
            let refresh = refresh.clone();
            move |_| refresh()
        });
        imp.minutes_spin.connect_value_changed(move |_| refresh());
        imp.minutes_spin.connect_output(|spin| {
            spin.set_text(&format!("{:02}", spin.value() as u32));
            glib::Propagation::Stop
        });
        // Stepping minutes past :55 or below :00 carries into the hours.
        imp.minutes_spin.connect_wrapped(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |minutes| {
                let hours = &page.imp().hours_spin;
                let delta = if minutes.value() < 30.0 { 1.0 } else { -1.0 };
                hours.set_value(hours.value() + delta);
            }
        ));

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

    #[cfg(debug_assertions)]
    pub fn show_custom_duration(&self) {
        self.imp().duration_group.set_active_name(Some("custom"));
    }

    fn preview(&self, id: &str) {
        let Some(win) = self.window() else { return };
        let Some(status) = win.status() else { return };
        if let Some(list) = status.state.list(id) {
            crate::preview::present(&win, list, &status);
        }
    }

    fn window(&self) -> Option<Window> {
        self.root().and_downcast()
    }

    fn minutes(&self) -> u32 {
        let imp = self.imp();
        match imp.duration_group.active_name().as_deref() {
            Some("custom") | None => (imp.hours_spin.value() as u32) * 60 + imp.minutes_spin.value() as u32,
            Some(minutes) => minutes.parse().unwrap_or(60),
        }
    }

    fn selected_lists(&self) -> Vec<String> {
        self.imp().list_rows.borrow().iter().filter(|r| r.switch.is_active()).map(|r| r.id.clone()).collect()
    }

    fn update_freeze_button(&self) {
        let imp = self.imp();
        imp.custom_group.set_visible(imp.duration_group.active_name().as_deref() == Some("custom"));
        let minutes = self.minutes();
        imp.freeze_button.set_sensitive(minutes >= 1 && !self.selected_lists().is_empty());
    }

    fn start_freeze(&self) {
        let imp = self.imp();
        let lists = self.selected_lists();
        let seconds = u64::from(self.minutes()) * 60;
        let locked = imp.lock_freezes.get();
        let breaks = BREAKS[imp.breaks_row.selected() as usize];
        if let Some(win) = self.window() {
            win.spawn(move |client| async move { client.start_freeze(&lists, seconds, locked, breaks).await });
        }
    }

    pub fn set_status(&self, status: &Status) {
        let imp = self.imp();
        let lock = status.state.settings.lock_freezes;
        imp.lock_freezes.set(lock);
        imp.lock_hint.set_label(if lock {
            "Can’t be stopped until the timer ends · Change this in Settings"
        } else {
            "Can be stopped anytime · Change this in Settings"
        });
        self.rebuild_list_rows(status);

        let freeze = status.state.freeze.as_ref().filter(|f| f.ends_at > status.now);
        imp.freeze.replace(freeze.cloned());
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
                && rows.iter().zip(&status.state.lists).all(|(r, list)| r.id == list.id && r.row.title() == list.name)
        };
        if same {
            for (r, list) in imp.list_rows.borrow().iter().zip(&status.state.lists) {
                r.row.set_subtitle(&format::list_summary(list, status));
            }
            return;
        }

        for r in imp.list_rows.take() {
            imp.lists_group.remove(&r.row);
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
            // The switch picks the filter; the rest of the row opens a preview of it.
            let row = adw::ActionRow::builder()
                .title(&list.name)
                .subtitle(format::list_summary(list, status))
                .activatable(true)
                .tooltip_text("Show what this filter blocks")
                .build();
            let switch = gtk::Switch::builder()
                .active(!imp.unchecked.borrow().contains(&list.id))
                .valign(gtk::Align::Center)
                .build();
            switch.update_property(&[gtk::accessible::Property::Label(&format!("Block {}", list.name))]);
            row.add_suffix(&switch);
            let id = list.id.clone();
            row.connect_activated(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |_| page.preview(&id)
            ));
            let id = list.id.clone();
            switch.connect_active_notify(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |switch| {
                    let mut unchecked = page.imp().unchecked.borrow_mut();
                    if switch.is_active() {
                        unchecked.remove(&id);
                    } else {
                        unchecked.insert(id.clone());
                    }
                    drop(unchecked);
                    page.update_freeze_button();
                }
            ));
            imp.lists_group.add(&row);
            rows.push(ListRow { id: list.id.clone(), row, switch });
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
        let phase = imp.freeze.borrow().as_ref().filter(|f| f.breaks.is_some()).map(|f| f.phase(now));
        imp.phase_label.set_visible(phase.is_some());
        match phase {
            Some(Phase::OnBreak { until }) => imp
                .phase_label
                .set_label(&format!("On a break — blocks are back in {}", format::countdown(until - now))),
            Some(Phase::Working { until }) if until < end => {
                imp.phase_label.set_label(&format!("Next break in {}", format::countdown(until - now)))
            }
            _ => imp.phase_label.set_label("Last stretch — no more breaks"),
        }
        imp.ring.set_fraction(remaining as f64 / (end - start) as f64);
    }
}
