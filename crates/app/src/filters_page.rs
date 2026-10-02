use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use permafrost_common::community::{self, Category};
use permafrost_common::model::{Filters, Status};

use crate::format;
use crate::window::Window;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/atayoez/Permafrost/ui/filters-page.ui")]
    pub struct FiltersPage {
        #[template_child]
        pub page: TemplateChild<adw::PreferencesPage>,
        #[template_child]
        pub safe_search_row: TemplateChild<adw::SwitchRow>,

        pub source_rows: RefCell<Vec<(&'static str, adw::SwitchRow)>>,
        pub filters: RefCell<Filters>,
        /// Set while the page fills in its widgets, so that isn't saved back.
        pub loading: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FiltersPage {
        const NAME: &'static str = "PermafrostFiltersPage";
        type Type = super::FiltersPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for FiltersPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for FiltersPage {}
    impl BinImpl for FiltersPage {}
}

glib::wrapper! {
    pub struct FiltersPage(ObjectSubclass<imp::FiltersPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FiltersPage {
    fn setup(&self) {
        let imp = self.imp();
        imp.safe_search_row.connect_active_notify(glib::clone!(
            #[weak(rename_to = page)]
            self,
            move |row| {
                let on = row.is_active();
                page.edit(|filters| filters.safe_search = on);
            }
        ));

        let mut rows = Vec::new();
        for category in Category::ALL {
            let group = adw::PreferencesGroup::builder().title(category.title()).build();
            for source in community::SOURCES.iter().filter(|s| s.category == category) {
                let row = adw::SwitchRow::builder().title(source.name).subtitle_lines(2).build();
                let id = source.id;
                row.connect_active_notify(glib::clone!(
                    #[weak(rename_to = page)]
                    self,
                    move |row| {
                        let on = row.is_active();
                        page.edit(|filters| {
                            filters.community.retain(|c| c != id);
                            if on {
                                filters.community.push(id.to_owned());
                            }
                        });
                    }
                ));
                group.add(&row);
                rows.push((id, row));
            }
            imp.page.add(&group);
        }
        imp.source_rows.replace(rows);
    }

    fn window(&self) -> Option<Window> {
        self.root().and_downcast()
    }

    /// Changes the filters and saves them. The service sends the result back.
    fn edit(&self, change: impl FnOnce(&mut Filters)) {
        if self.imp().loading.get() {
            return;
        }
        let mut filters = self.imp().filters.borrow().clone();
        change(&mut filters);
        if let Some(win) = self.window() {
            win.spawn(move |client| async move { client.set_filters(&filters).await });
        }
    }

    pub fn set_status(&self, status: &Status) {
        let imp = self.imp();
        let filters = &status.state.filters;
        // While locked, filters can be turned on but not off.
        let locked = !status.locked_lists.is_empty();
        imp.loading.set(true);
        imp.filters.replace(filters.clone());

        imp.safe_search_row.set_active(filters.safe_search);
        imp.safe_search_row.set_sensitive(!(locked && filters.safe_search));

        for (id, row) in imp.source_rows.borrow().iter() {
            let source = community::find(id).expect("rows come from SOURCES");
            let on = filters.community.iter().any(|c| c == id);
            row.set_active(on);
            row.set_sensitive(!(locked && on));
            let size = match status.community_sizes.get(*id) {
                Some(n) => format!("{} sites from {}", format::thousands(*n), source.project),
                None if on => "Downloading…".to_owned(),
                None => format!("From {}", source.project),
            };
            row.set_subtitle(&format!("{}\n{size}", source.description));
        }
        imp.loading.set(false);
    }
}
