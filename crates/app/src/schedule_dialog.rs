//! Creating and editing a schedule.

use adw::prelude::*;
use gtk::glib;
use permafrost_common::model::{Schedule, Status};

use crate::window::Window;

const DAYS: [(&str, &str); 7] = [
    ("M", "Monday"),
    ("T", "Tuesday"),
    ("W", "Wednesday"),
    ("T", "Thursday"),
    ("F", "Friday"),
    ("S", "Saturday"),
    ("S", "Sunday"),
];

/// A row with `HH : MM` spin buttons.
fn time_row(title: &str, minutes: u16) -> (adw::ActionRow, gtk::SpinButton, gtk::SpinButton) {
    let spin = |upper: f64, step: f64, value: f64, label: &str| {
        let spin = gtk::SpinButton::builder()
            .adjustment(&gtk::Adjustment::new(value, 0.0, upper, step, step, 0.0))
            .orientation(gtk::Orientation::Vertical)
            .wrap(true)
            .numeric(true)
            .valign(gtk::Align::Center)
            .build();
        spin.connect_output(|spin| {
            spin.set_text(&format!("{:02}", spin.value() as u32));
            glib::Propagation::Stop
        });
        spin.update_property(&[gtk::accessible::Property::Label(label)]);
        spin
    };
    let hours = spin(23.0, 1.0, f64::from(minutes / 60), &format!("{title} hour"));
    let mins = spin(55.0, 5.0, f64::from(minutes % 60 / 5 * 5), &format!("{title} minute"));
    let row = adw::ActionRow::builder().title(title).build();
    let box_ = gtk::Box::builder().spacing(6).valign(gtk::Align::Center).build();
    box_.append(&hours);
    box_.append(&gtk::Label::new(Some(":")));
    box_.append(&mins);
    row.add_suffix(&box_);
    (row, hours, mins)
}

pub fn present(win: &Window, status: &Status, existing: Option<Schedule>) {
    let editing = existing.is_some();
    let schedule = existing.unwrap_or_else(|| Schedule { name: "Work Hours".into(), ..Default::default() });

    let dialog = adw::Dialog::builder()
        .title(if editing { "Edit Schedule" } else { "New Schedule" })
        .content_width(460)
        .build();

    let header = adw::HeaderBar::builder().show_start_title_buttons(false).show_end_title_buttons(false).build();
    let cancel = gtk::Button::with_mnemonic("_Cancel");
    let save = gtk::Button::builder().use_underline(true).label("_Save").css_classes(["suggested-action"]).build();
    header.pack_start(&cancel);
    header.pack_end(&save);
    cancel.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        move |_| {
            dialog.close();
        }
    ));

    let page = adw::PreferencesPage::new();

    let name_group = adw::PreferencesGroup::new();
    let name = adw::EntryRow::builder().title("Name").text(&schedule.name).build();
    name_group.add(&name);
    page.add(&name_group);

    let days_group = adw::PreferencesGroup::builder().title("Days").build();
    let days_box = gtk::Box::builder().spacing(6).halign(gtk::Align::Center).build();
    let day_buttons: Vec<gtk::ToggleButton> = DAYS
        .iter()
        .zip(schedule.days)
        .map(|((short, full), on)| {
            let button = gtk::ToggleButton::builder()
                .label(*short)
                .active(on)
                .tooltip_text(*full)
                .width_request(44)
                .height_request(44)
                .css_classes(["circular"])
                .build();
            button.update_property(&[gtk::accessible::Property::Label(full)]);
            days_box.append(&button);
            button
        })
        .collect();
    days_group.add(&days_box);
    page.add(&days_group);

    let time_group = adw::PreferencesGroup::builder()
        .title("Time")
        .description("An end before the start runs overnight. All-day schedules can’t be locked.")
        .build();
    let all_day = adw::SwitchRow::builder().title("All Day").active(schedule.all_day).build();
    let (start_row, start_h, start_m) = time_row("Starts", schedule.start);
    let (end_row, end_h, end_m) = time_row("Ends", schedule.end);
    time_group.add(&all_day);
    time_group.add(&start_row);
    time_group.add(&end_row);
    for row in [&start_row, &end_row] {
        all_day.bind_property("active", row, "visible").invert_boolean().sync_create().build();
    }
    page.add(&time_group);

    let lists_group = adw::PreferencesGroup::builder().title("Block").build();
    let list_rows: Vec<(String, adw::SwitchRow)> = status
        .state
        .lists
        .iter()
        .map(|list| {
            let row = adw::SwitchRow::builder().title(&list.name).active(schedule.lists.contains(&list.id)).build();
            lists_group.add(&row);
            (list.id.clone(), row)
        })
        .collect();
    page.add(&lists_group);

    let lock_group = adw::PreferencesGroup::new();
    let locked = adw::SwitchRow::builder()
        .title("Lock While Running")
        .subtitle("Can’t be turned off or edited until it ends")
        .active(schedule.locked && !schedule.all_day)
        .build();
    // All-day schedules may never end, so they can't be locked.
    all_day.bind_property("active", &locked, "sensitive").invert_boolean().sync_create().build();
    all_day.connect_active_notify(glib::clone!(
        #[weak]
        locked,
        move |all_day| {
            if all_day.is_active() {
                locked.set_active(false);
            }
        }
    ));
    lock_group.add(&locked);
    page.add(&lock_group);

    if editing {
        let delete_group = adw::PreferencesGroup::new();
        let delete = adw::ButtonRow::builder().title("_Delete Schedule").use_underline(true).css_classes(["destructive-action"]).build();
        let id = schedule.id.clone();
        delete.connect_activated(glib::clone!(
            #[weak]
            win,
            #[weak]
            dialog,
            move |_| {
                let id = id.clone();
                win.spawn(move |client| async move { client.delete_schedule(&id).await });
                dialog.close();
            }
        ));
        delete_group.add(&delete);
        page.add(&delete_group);
    }

    save.connect_clicked(glib::clone!(
        #[weak]
        win,
        #[weak]
        dialog,
        move |_| {
            let to_minutes = |h: &gtk::SpinButton, m: &gtk::SpinButton| (h.value() as u16) * 60 + m.value() as u16;
            let mut updated = schedule.clone();
            updated.name = name.text().trim().to_owned();
            for (day, button) in updated.days.iter_mut().zip(&day_buttons) {
                *day = button.is_active();
            }
            updated.start = to_minutes(&start_h, &start_m);
            updated.end = to_minutes(&end_h, &end_m);
            updated.lists = list_rows.iter().filter(|(_, row)| row.is_active()).map(|(id, _)| id.clone()).collect();
            updated.all_day = all_day.is_active();
            updated.locked = locked.is_active() && !updated.all_day;
            if updated.name.is_empty() {
                win.toast("Give the schedule a name");
                return;
            }
            if updated.lists.is_empty() {
                win.toast("Choose at least one block list");
                return;
            }
            win.spawn(move |client| async move { client.save_schedule(&updated).await.map(|_| ()) });
            dialog.close();
        }
    ));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&page));
    dialog.set_child(Some(&toolbar));
    dialog.set_default_widget(Some(&save));
    dialog.present(Some(win));
}
