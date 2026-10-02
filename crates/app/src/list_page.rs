use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use permafrost_common::model::{BlockList, Status};
use permafrost_common::{domain, presets};

use crate::app_picker;
use crate::window::Window;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/atayoez/Permafrost/ui/list-page.ui")]
    pub struct ListPage {
        #[template_child]
        pub sites_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub preset_button: TemplateChild<gtk::MenuButton>,
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
            klass.install_action("list.add-preset", Some(glib::VariantTy::STRING), |page, _, param| {
                if let Some(preset) = param.and_then(|p| p.get::<String>()).and_then(|id| presets::find(&id)) {
                    page.edit(|list| preset.merge_into(list));
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

        let menu = gio::Menu::new();
        for preset in presets::PRESETS {
            let item = gio::MenuItem::new(Some(preset.name), None);
            item.set_action_and_target_value(Some("list.add-preset"), Some(&preset.id.to_variant()));
            menu.append_item(&item);
        }
        imp.preset_button.set_menu_model(Some(&menu));

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
        if imp.list.borrow().as_ref() == Some(list) && imp.locked.get() == locked {
            return;
        }
        imp.list.replace(Some(list.clone()));
        imp.locked.set(locked);

        for (group, row) in imp.rows.take() {
            group.remove(&row);
        }
        let mut rows = Vec::new();
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
