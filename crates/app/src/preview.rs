//! A read-only look at what a filter blocks.

use adw::prelude::*;
use gtk::glib;
use permafrost_common::community;
use permafrost_common::model::{BlockList, Status};

use crate::format;
use crate::window::{Section, Window};

pub fn present(win: &Window, list: &BlockList, status: &Status) {
    let dialog = adw::Dialog::builder().title(&list.name).content_width(440).content_height(600).build();

    let header = adw::HeaderBar::new();
    let edit = gtk::Button::with_mnemonic("_Edit");
    header.pack_start(&edit);
    let id = list.id.clone();
    edit.connect_clicked(glib::clone!(
        #[weak]
        win,
        #[weak]
        dialog,
        move |_| {
            dialog.close();
            win.show_section(Section::List(id.clone()));
            win.show_content();
        }
    ));

    let page = adw::PreferencesPage::builder().description(format::list_summary(list, status)).build();

    if !list.community.is_empty() {
        let group = adw::PreferencesGroup::builder().title("Community Lists").build();
        for id in &list.community {
            let Some(source) = community::find(id) else { continue };
            let size = match status.community_sizes.get(id) {
                Some(n) => format!("{} sites from {}", format::thousands(*n), source.project),
                None => "Downloading…".to_owned(),
            };
            group.add(&adw::ActionRow::builder().title(source.name).subtitle(&size).build());
        }
        page.add(&group);
    }

    let sites = adw::PreferencesGroup::builder().title("Websites").build();
    if list.sites.is_empty() {
        sites.set_description(Some("None"));
    }
    for site in &list.sites {
        sites.add(&adw::ActionRow::builder().title(site).build());
    }
    page.add(&sites);

    let apps = adw::PreferencesGroup::builder().title("Apps").build();
    if list.apps.is_empty() {
        apps.set_description(Some("None"));
    }
    for app_id in &list.apps {
        let info = gio_unix::DesktopAppInfo::new(&format!("{app_id}.desktop"));
        let name = info.as_ref().map(|i| i.display_name().to_string()).unwrap_or_else(|| app_id.clone());
        let row = adw::ActionRow::builder().title(&name).subtitle(app_id).build();
        let icon = gtk::Image::builder().pixel_size(32).css_classes(["lowres-icon"]).build();
        match info.and_then(|i| i.icon()) {
            Some(gicon) => icon.set_from_gicon(&gicon),
            None => icon.set_icon_name(Some("application-x-executable-symbolic")),
        }
        row.add_prefix(&icon);
        apps.add(&row);
    }
    page.add(&apps);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&page));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(win));
}
