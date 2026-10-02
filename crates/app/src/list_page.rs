use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use permafrost_common::model::{BlockList, Status};
use permafrost_common::community::{self, Category};
use permafrost_common::domain;

use crate::app_picker;
use crate::format;
use crate::window::Window;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/atayoez/Permafrost/ui/list-page.ui")]
    pub struct ListPage {
        #[template_child]
        pub sites_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub limit_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub limit_minutes_row: TemplateChild<adw::SpinRow>,
        #[template_child]
        pub community_group: TemplateChild<adw::PreferencesGroup>,
        /// Set while the page fills in its widgets, so that isn't saved back.
        pub loading: Cell<bool>,
        #[template_child]
        pub add_community_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub site_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub apps_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub add_app_row: TemplateChild<adw::ButtonRow>,

        pub list: RefCell<Option<BlockList>>,
        pub locked: Cell<bool>,
        pub rows: RefCell<Vec<(adw::PreferencesGroup, adw::ActionRow)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ListPage {
        const NAME: &'static str = "PermafrostListPage";
        type Type = super::ListPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.install_action("list.add-community", Some(glib::VariantTy::STRING), |page, _, param| {
                if let Some(id) = param.and_then(|p| p.get::<String>()) {
                    page.edit(|list| {
                        if !list.community.contains(&id) {
                            list.community.push(id);
                        }
                    });
                }
            });
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for ListPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for ListPage {}
    impl BinImpl for ListPage {}
}

glib::wrapper! {
    pub struct ListPage(ObjectSubclass<imp::ListPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ListPage {
    fn setup(&self) {
        let imp = self.imp();

        imp.limit_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.save_limit()
        ));
        imp.limit_minutes_row.connect_value_notify(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.save_limit()
        ));

        imp.site_entry.connect_apply(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |entry| {
                let text = entry.text();
                match domain::normalize(&text) {
                    Some(site) => {
                        page.edit(|list| {
                            if !list.sites.contains(&site) {
                                list.sites.push(site);
                            }
                        });
                        entry.set_text("");
                    }
                    None => {
                        if let Some(win) = page.window() {
                            win.toast(&format!("“{}” isn’t a website address", text.trim()));
                        }
                    }
                }
            }
        ));

        imp.add_app_row.connect_activated(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| {
                let Some(win) = page.window() else { return };
                let page = page.downgrade();
                app_picker::present(&win, move |app_id| {
                    if let Some(page) = page.upgrade() {
                        page.edit(|list| {
                            if !list.apps.contains(&app_id) {
                                list.apps.push(app_id);
                            }
                        });
                    }
                });
            }
        ));
    }

    fn window(&self) -> Option<Window> {
        self.root().and_downcast()
    }

    fn save_limit(&self) {
        let imp = self.imp();
        if imp.loading.get() {
            return;
        }
        let limit = imp.limit_row.is_active().then(|| imp.limit_minutes_row.value() as u32);
        self.edit(|list| list.daily_limit = limit);
    }

    /// Changes the shown list and saves it. The service sends the result back.
    fn edit(&self, change: impl FnOnce(&mut BlockList)) {
        let Some(mut list) = self.imp().list.borrow().clone() else { return };
        change(&mut list);
        if let Some(win) = self.window() {
            win.spawn(move |client| async move { client.save_list(&list).await.map(|_| ()) });
        }
    }

    pub fn set_list(&self, list: &BlockList, status: &Status) {
        let imp = self.imp();
        let locked = status.locked_lists.contains(&list.id);
        self.update_limit(list, status, locked);
        if imp.list.borrow().as_ref() == Some(list) && imp.locked.get() == locked {
            return;
        }
        imp.list.replace(Some(list.clone()));
        imp.locked.set(locked);

        for (group, row) in imp.rows.take() {
            group.remove(&row);
        }
        let mut rows = Vec::new();
        for id in &list.community {
            let Some(source) = community::find(id) else { continue };
            let size = match status.community_sizes.get(id) {
                Some(n) => format!("{} sites from {}", format::thousands(*n), source.project),
                None => "Downloading…".to_owned(),
            };
            let row = adw::ActionRow::builder().title(source.name).subtitle(&size).build();
            let id = id.clone();
            row.add_suffix(&self.remove_button(&format!("Remove {}", source.name), move |list| {
                list.community.retain(|c| *c != id)
            }));
            imp.community_group.add(&row);
            rows.push((imp.community_group.get(), row));
        }
        imp.add_community_button.set_menu_model(Some(&community_menu(list)));
        for site in &list.sites {
            let row = adw::ActionRow::builder().title(site).build();
            let site = site.clone();
            row.add_suffix(&self.remove_button(&format!("Remove {site}"), move |list| list.sites.retain(|s| *s != site)));
            imp.sites_group.add(&row);
            rows.push((imp.sites_group.get(), row));
        }

        imp.apps_group.remove(&*imp.add_app_row);
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
            let id = app_id.clone();
            row.add_suffix(&self.remove_button(&format!("Remove {name}"), move |list| list.apps.retain(|a| *a != id)));
            imp.apps_group.add(&row);
            rows.push((imp.apps_group.get(), row));
        }
        imp.apps_group.add(&*imp.add_app_row);
        imp.rows.replace(rows);

    }

    fn update_limit(&self, list: &BlockList, status: &Status, locked: bool) {
        let imp = self.imp();
        imp.loading.set(true);
        imp.limit_row.set_active(list.daily_limit.is_some());
        let minutes = list.daily_limit.unwrap_or(30);
        // While locked, a limit can be lowered but not raised or removed.
        let adjustment = imp.limit_minutes_row.adjustment();
        adjustment.set_upper(if locked && list.daily_limit.is_some() { f64::from(minutes) } else { 720.0 });
        imp.limit_minutes_row.set_value(f64::from(minutes));
        imp.limit_row.set_sensitive(!(locked && list.daily_limit.is_some()));
        let used = format::usage_today(status, &list.id);
        imp.limit_row.set_subtitle(&if status.limit_reached.contains(&list.id) {
            "Used up for today — blocked until midnight".to_owned()
        } else if list.daily_limit.is_some() {
            format!("Used {} today on its websites and apps", format::duration(used))
        } else {
            "Block it for the rest of the day once its time is used up".to_owned()
        });
        imp.loading.set(false);
    }

    fn remove_button(&self, label: &str, remove: impl Fn(&mut BlockList) + 'static) -> gtk::Button {
        let locked = self.imp().locked.get();
        let button = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(if locked { "Can’t remove while frozen" } else { label })
            .sensitive(!locked)
            .css_classes(["flat"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(label)]);
        button.connect_clicked(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.edit(&remove)
        ));
        button
    }
}

/// Community lists not in `list` yet, grouped by category.
fn community_menu(list: &BlockList) -> gio::Menu {
    let menu = gio::Menu::new();
    for category in Category::ALL {
        let section = gio::Menu::new();
        for source in community::SOURCES.iter().filter(|s| s.category == category && !list.community.iter().any(|c| c == s.id)) {
            let item = gio::MenuItem::new(Some(source.name), None);
            item.set_action_and_target_value(Some("list.add-community"), Some(&source.id.to_variant()));
            section.append_item(&item);
        }
        if section.n_items() > 0 {
            menu.append_section(Some(category.title()), &section);
        }
    }
    menu
}
