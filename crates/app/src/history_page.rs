//! Focus time, streaks and what got blocked, day by day.

use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use chrono::Local;
use gtk::glib;
use permafrost_common::model::{DayStats, Status, day_key, streak};

use crate::format;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct HistoryPage {
        pub today: gtk::Label,
        pub week: gtk::Label,
        pub streak: gtk::Label,
        pub days_group: adw::PreferencesGroup,
        pub day_rows: RefCell<Vec<adw::ActionRow>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HistoryPage {
        const NAME: &'static str = "PermafrostHistoryPage";
        type Type = super::HistoryPage;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for HistoryPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for HistoryPage {}
    impl BinImpl for HistoryPage {}
}

glib::wrapper! {
    pub struct HistoryPage(ObjectSubclass<imp::HistoryPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// A big number with a caption under it.
fn stat_card(value: &gtk::Label, caption: &str) -> gtk::Box {
    value.add_css_class("title-2");
    value.add_css_class("numeric");
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .css_classes(["card"])
        .build();
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(12)
        .margin_end(12)
        .build();
    inner.append(value);
    inner.append(&gtk::Label::builder().label(caption).css_classes(["dimmed", "caption"]).build());
    card.append(&inner);
    card
}

impl HistoryPage {
    fn setup(&self) {
        let imp = self.imp();
        let page = adw::PreferencesPage::new();

        let summary = adw::PreferencesGroup::new();
        let cards = gtk::Box::builder().spacing(12).homogeneous(true).build();
        cards.append(&stat_card(&imp.today, "Today"));
        cards.append(&stat_card(&imp.week, "Last 7 Days"));
        cards.append(&stat_card(&imp.streak, "Day Streak"));
        summary.add(&cards);
        page.add(&summary);

        imp.days_group.set_title("Last 7 Days");
        imp.days_group.set_description(Some(
            "Focus time is time with anything blocked. A day joins your streak with at least 15 minutes.",
        ));
        page.add(&imp.days_group);
        self.set_child(Some(&page));
    }

    pub fn set_status(&self, status: &Status) {
        let imp = self.imp();
        let history = &status.state.history;
        let today = Local::now().date_naive();
        let days: Vec<_> = (0..7).map(|ago| today - chrono::Duration::days(ago)).collect();
        let stats = |date| history.get(&day_key(date)).cloned().unwrap_or_default();

        let week: u64 = days.iter().map(|d| stats(*d).focus_seconds).sum();
        imp.today.set_label(&format::duration(stats(today).focus_seconds));
        imp.week.set_label(&format::duration(week));
        let streak = streak(history, today);
        imp.streak.set_label(&if streak == 1 { "1 day".to_owned() } else { format!("{streak} days") });

        for row in imp.day_rows.take() {
            imp.days_group.remove(&row);
        }
        let most = days.iter().map(|d| stats(*d).focus_seconds).max().unwrap_or(0).max(1);
        let mut rows = Vec::new();
        for (ago, date) in days.iter().enumerate() {
            let DayStats { focus_seconds, freezes, apps_closed, .. } = stats(*date);
            let title = match ago {
                0 => "Today".to_owned(),
                1 => "Yesterday".to_owned(),
                _ => date.format("%A").to_string(),
            };
            let mut details = Vec::new();
            if freezes > 0 {
                details.push(if freezes == 1 { "1 freeze".to_owned() } else { format!("{freezes} freezes") });
            }
            if apps_closed > 0 {
                details.push(if apps_closed == 1 { "1 app closed".to_owned() } else { format!("{apps_closed} apps closed") });
            }
            let row = adw::ActionRow::builder().title(&title).build();
            if !details.is_empty() {
                row.set_subtitle(&details.join(" · "));
            }
            let bar = gtk::LevelBar::builder()
                .min_value(0.0)
                .max_value(1.0)
                .value(focus_seconds as f64 / most as f64)
                .width_request(120)
                .valign(gtk::Align::Center)
                .build();
            bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_LOW));
            bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_HIGH));
            bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_FULL));
            bar.update_property(&[gtk::accessible::Property::Label(&format!("{title}: {}", format::duration(focus_seconds)))]);
            row.add_suffix(&bar);
            row.add_suffix(
                &gtk::Label::builder()
                    .label(format::duration(focus_seconds))
                    .width_chars(9)
                    .xalign(1.0)
                    .css_classes(["numeric", "dimmed"])
                    .build(),
            );
            imp.days_group.add(&row);
            rows.push(row);
        }
        imp.day_rows.replace(rows);
    }
}
