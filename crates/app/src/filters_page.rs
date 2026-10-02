use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use permafrost_common::model::Status;

use crate::format;
use crate::window::{Section, Window};

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/atayoez/Permafrost/ui/filters-page.ui")]
    pub struct FiltersPage {
        #[template_child]
        pub lists_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub add_row: TemplateChild<adw::ButtonRow>,

        pub list_rows: RefCell<Vec<adw::ActionRow>>,
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

    impl ObjectImpl for FiltersPage {}

    impl WidgetImpl for FiltersPage {}
    impl BinImpl for FiltersPage {}
}

glib::wrapper! {
    pub struct FiltersPage(ObjectSubclass<imp::FiltersPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FiltersPage {
    /// Your filters as rows that open the full list; choosing what to block
    /// happens on the Freeze page.
    pub fn set_status(&self, status: &Status) {
        let imp = self.imp();
        for row in imp.list_rows.take() {
            imp.lists_group.remove(&row);
        }
        imp.lists_group.remove(&*imp.add_row);
        let mut rows = Vec::new();
        for list in &status.state.lists {
            let row = adw::ActionRow::builder()
                .title(&list.name)
                .subtitle(format::list_summary(list, status))
                .activatable(true)
                .build();
            if status.locked_lists.contains(&list.id) {
                row.add_suffix(&gtk::Image::builder().icon_name("system-lock-screen-symbolic").tooltip_text("Frozen").build());
            }
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            let id = list.id.clone();
            row.connect_activated(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |_| {
                    if let Some(win) = page.window() {
                        win.show_section(Section::List(id.clone()));
                    }
                }
            ));
            imp.lists_group.add(&row);
            rows.push(row);
        }
        imp.lists_group.add(&*imp.add_row);
        imp.list_rows.replace(rows);
    }

    fn window(&self) -> Option<Window> {
        self.root().and_downcast()
    }
}
