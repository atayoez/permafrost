//! Choosing an installed app to block.

use adw::prelude::*;
use gtk::{gio, glib};

use crate::window::Window;

/// Shows installed apps; `on_pick` gets the chosen app's ID (no `.desktop`).
pub fn present(win: &Window, on_pick: impl Fn(String) + 'static) {
    let mut apps: Vec<gio::AppInfo> = gio::AppInfo::all().into_iter().filter(|a| a.should_show()).collect();
    apps.sort_by_key(|a| a.display_name().to_lowercase());

    let dialog = adw::Dialog::builder().title("Add App").content_width(420).content_height(560).build();
    let search = gtk::SearchEntry::builder().placeholder_text("Search apps").hexpand(true).build();
    let header = adw::HeaderBar::new();
    let search_bar = gtk::SearchBar::builder().search_mode_enabled(true).child(&search).build();
    search_bar.connect_entry(&search);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .valign(gtk::Align::Start)
        .build();
    for app in &apps {
        let Some(id) = app.id() else { continue };
        let id = id.trim_end_matches(".desktop").to_owned();
        let row = adw::ActionRow::builder().title(app.display_name().as_str()).subtitle(&id).activatable(true).build();
        let icon = gtk::Image::builder().pixel_size(32).css_classes(["lowres-icon"]).build();
        match app.icon() {
            Some(gicon) => icon.set_from_gicon(&gicon),
            None => icon.set_icon_name(Some("application-x-executable-symbolic")),
        }
        row.add_prefix(&icon);
        list.append(&row);
    }

    list.set_filter_func(glib::clone!(
        #[weak]
        search,
        #[upgrade_or]
        true,
        move |row| {
            let query = search.text().to_lowercase();
            let Some(row) = row.downcast_ref::<adw::ActionRow>() else { return true };
            query.is_empty()
                || row.title().to_lowercase().contains(&query)
                || row.subtitle().is_some_and(|s| s.to_lowercase().contains(&query))
        }
    ));
    search.connect_search_changed(glib::clone!(
        #[weak]
        list,
        move |_| list.invalidate_filter()
    ));

    list.connect_row_activated(glib::clone!(
        #[weak]
        dialog,
        move |_, row| {
            if let Some(id) = row.downcast_ref::<adw::ActionRow>().and_then(|r| r.subtitle()) {
                on_pick(id.to_string());
            }
            dialog.close();
        }
    ));

    let clamp = adw::Clamp::builder()
        .child(&list)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let scrolled = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&clamp).build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&search_bar);
    toolbar.set_content(Some(&scrolled));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(win));
    search.grab_focus();
}
