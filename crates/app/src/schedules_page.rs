use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use permafrost_common::model::{Schedule, Status};

use crate::format;
use crate::schedule_dialog;
use crate::window::Window;

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/atayoez/Permafrost/ui/schedules-page.ui")]
    pub struct SchedulesPage {
        #[template_child]
        pub schedules_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub new_schedule_row: TemplateChild<adw::ButtonRow>,

        pub rows: RefCell<Vec<adw::ActionRow>>,
        pub status: RefCell<Option<Rc<Status>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SchedulesPage {
        const NAME: &'static str = "PermafrostSchedulesPage";
        type Type = super::SchedulesPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SchedulesPage {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            self.new_schedule_row.connect_activated(glib::clone!(
                #[weak]
                obj,
                move |_| obj.open_editor(None)
            ));
        }
    }

    impl WidgetImpl for SchedulesPage {}
    impl BinImpl for SchedulesPage {}
}

glib::wrapper! {
    pub struct SchedulesPage(ObjectSubclass<imp::SchedulesPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SchedulesPage {
    fn window(&self) -> Option<Window> {
        self.root().and_downcast()
    }

    fn open_editor(&self, id: Option<&str>) {
        let (Some(win), Some(status)) = (self.window(), self.imp().status.borrow().clone()) else { return };
        let schedule = id.and_then(|id| status.state.schedule(id)).cloned();
        schedule_dialog::present(&win, &status, schedule);
    }

    pub fn set_status(&self, status: &Status) {
        let imp = self.imp();
        imp.status.replace(Some(Rc::new(status.clone())));
        for row in imp.rows.take() {
            imp.schedules_group.remove(&row);
        }
        imp.schedules_group.remove(&*imp.new_schedule_row);

        let mut rows = Vec::new();
        for schedule in &status.state.schedules {
            let names: Vec<String> =
                schedule.lists.iter().filter_map(|id| status.state.list(id)).map(|l| l.name.clone()).collect();
            let frozen = schedule.locked && status.running_schedules.contains(&schedule.id);
            let row = adw::ActionRow::builder()
                .title(&schedule.name)
                .subtitle(format::schedule_summary(schedule, &names))
                .activatable(!frozen)
                .build();

            let switch = gtk::Switch::builder()
                .active(schedule.enabled)
                .valign(gtk::Align::Center)
                .sensitive(!frozen)
                .build();
            switch.update_property(&[gtk::accessible::Property::Label(&schedule.name)]);
            let toggled = schedule.clone();
            switch.connect_state_set(glib::clone!(
                #[weak(rename_to = page)]
                self,
                #[upgrade_or]
                glib::Propagation::Stop,
                move |_, on| {
                    let schedule = Schedule { enabled: on, ..toggled.clone() };
                    if let Some(win) = page.window() {
                        win.spawn(move |client| async move { client.save_schedule(&schedule).await.map(|_| ()) });
                    }
                    glib::Propagation::Proceed
                }
            ));
            if frozen {
                row.add_suffix(&gtk::Image::builder().icon_name("system-lock-screen-symbolic").tooltip_text("Locked until it ends").build());
            }
            row.add_suffix(&switch);

            let id = schedule.id.clone();
            row.connect_activated(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |_| page.open_editor(Some(&id))
            ));
            imp.schedules_group.add(&row);
            rows.push(row);
        }
        imp.schedules_group.add(&*imp.new_schedule_row);
        imp.rows.replace(rows);
    }
}
