use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use futures_util::StreamExt;
use gtk::{gio, glib};
use permafrost_common::model::{BlockList, Phase, Schedule, Status};

use crate::client::Client;
use crate::format;
use crate::filters_page::FiltersPage;
use crate::freeze_page::FreezePage;
use crate::history_page::HistoryPage;
use crate::list_page::ListPage;
use crate::schedules_page::SchedulesPage;
use crate::settings_page::SettingsPage;

const APP_SYMBOLIC: &str = "io.github.atayoez.Permafrost-symbolic";
const REMIND_BEFORE_SCHEDULE: u32 = 5;
const HOLD_BEFORE_SCHEDULE: u32 = 10;

/// Enabled schedules that start within `minutes`, with the minutes left.
fn upcoming_schedules(status: &Status, minutes: u32) -> Vec<(&Schedule, u32)> {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    let today = now.weekday().num_days_from_monday() as usize;
    let minute = now.hour() * 60 + now.minute();
    status
        .state
        .schedules
        .iter()
        .filter(|s| s.enabled && !status.running_schedules.contains(&s.id))
        .filter_map(|s| {
            let start = if s.all_day { 0 } else { u32::from(s.start) };
            // Starting later today, or just after midnight tomorrow.
            let (day, until) =
                if start > minute { (today, start - minute) } else { ((today + 1) % 7, start + 24 * 60 - minute) };
            (s.days[day] && until <= minutes).then_some((s, until))
        })
        .collect()
}

/// The sidebar row for a section; a list shows under Filters.
fn index_of(section: &Section) -> u32 {
    match section {
        Section::Freeze => 0,
        Section::Schedules => 1,
        Section::Filters | Section::List(_) => 2,
        Section::History => 3,
        Section::Settings => 4,
    }
}

/// What the content pane shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Freeze,
    Schedules,
    Filters,
    History,
    Settings,
    List(String),
}

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/atayoez/Permafrost/ui/window.ui")]
    pub struct Window {
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub split_view: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub sidebar: TemplateChild<adw::Sidebar>,
        #[template_child]
        pub content_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub list_menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub back_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub freeze_page: TemplateChild<FreezePage>,
        #[template_child]
        pub schedules_page: TemplateChild<SchedulesPage>,
        #[template_child]
        pub filters_page: TemplateChild<FiltersPage>,
        #[template_child]
        pub list_page: TemplateChild<ListPage>,
        #[template_child]
        pub settings_page: TemplateChild<SettingsPage>,
        #[template_child]
        pub history_page: TemplateChild<HistoryPage>,

        pub client: RefCell<Option<Client>>,
        pub status: RefCell<Option<Rc<Status>>>,
        pub section: RefCell<Section>,
        pub freeze_suffix: RefCell<Option<gtk::Label>>,
        /// Set while the sidebar is rebuilt, so selection changes aren't navigation.
        pub rebuilding: Cell<bool>,
        pub hold: RefCell<Option<gio::ApplicationHoldGuard>>,
        pub background_requested: Cell<bool>,
        pub background_message: RefCell<String>,
        /// Whether the freeze was on a break at the last tick, to announce changes.
        pub on_break: Cell<Option<bool>>,
        /// Schedule runs already announced, as `<id>-<date>-<start>`.
        pub reminded: RefCell<std::collections::HashSet<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "PermafrostWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            FreezePage::ensure_type();
            SchedulesPage::ensure_type();
            FiltersPage::ensure_type();
            SettingsPage::ensure_type();
            HistoryPage::ensure_type();
            ListPage::ensure_type();
            klass.bind_template();

            klass.install_action("win.new-list", None, |win, _, _| win.prompt_list_name(None));
            klass.install_action("win.rename-list", None, |win, _, _| {
                if let Some(list) = win.selected_list() {
                    win.prompt_list_name(Some(list));
                }
            });
            klass.install_action("win.delete-list", None, |win, _, _| win.confirm_delete_list());
            klass.install_action("win.preferences", None, |win, _, _| win.show_section(Section::Settings));
            klass.install_action("win.reconnect", None, |win, _, _| win.connect_service());
            klass.install_action("win.show-filters", None, |win, _, _| win.show_section(Section::Filters));
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Window {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    pub fn new(app: &adw::Application) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    fn setup(&self) {
        let imp = self.imp();
        imp.sidebar.connect_selected_notify(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |sidebar| win.on_sidebar_selected(sidebar.selected(), false)
        ));
        imp.sidebar.connect_activated(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_, index| win.on_sidebar_selected(index, true)
        ));

        glib::timeout_add_seconds_local(
            1,
            glib::clone!(
                #[weak(rename_to = win)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    win.tick();
                    glib::ControlFlow::Continue
                }
            ),
        );

        self.connect_service();

        #[cfg(debug_assertions)]
        self.setup_debug_screenshot();
    }

    /// Debug builds only: `PERMAFROST_SCREENSHOT=out.png` (and optionally
    /// `PERMAFROST_SECTION=schedules|list:<id>`) renders the window to a PNG and quits.
    #[cfg(debug_assertions)]
    fn setup_debug_screenshot(&self) {
        let Ok(path) = std::env::var("PERMAFROST_SCREENSHOT") else { return };
        glib::timeout_add_seconds_local_once(
            2,
            glib::clone!(
                #[weak(rename_to = win)]
                self,
                move || {
                    match std::env::var("PERMAFROST_SECTION").as_deref() {
                        Ok("schedules") => win.show_section(Section::Schedules),
                        Ok("filters") => win.show_section(Section::Filters),
                        Ok("custom") => win.imp().freeze_page.show_custom_duration(),
                        Ok("pomodoro") => win.imp().freeze_page.show_pomodoro(),
                        Ok("settings") => win.show_section(Section::Settings),
                        Ok("history") => win.show_section(Section::History),
                        Ok(s) if s.starts_with("preview:") => {
                            if let Some(status) = win.status()
                                && let Some(list) = status.state.list(&s[8..])
                            {
                                crate::preview::present(&win, list, &status);
                            }
                        }
                        Ok(s) if s.starts_with("list:") => win.show_section(Section::List(s[5..].to_owned())),
                        _ => {}
                    }
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(600),
                        glib::clone!(
                            #[weak]
                            win,
                            move || {
                                let (w, h) = (win.width(), win.height());
                                let paintable = gtk::WidgetPaintable::new(Some(&win));
                                let snapshot = gtk::Snapshot::new();
                                paintable.snapshot(&snapshot, f64::from(w), f64::from(h));
                                if let (Some(node), Some(renderer)) = (snapshot.to_node(), win.renderer()) {
                                    let texture = renderer.render_texture(node, None);
                                    if let Err(e) = texture.save_to_png(&path) {
                                        eprintln!("screenshot failed: {e}");
                                    }
                                }
                                if let Some(app) = win.application() {
                                    app.quit();
                                }
                            }
                        ),
                    );
                }
            ),
        );
    }

    pub fn client(&self) -> Option<Client> {
        self.imp().client.borrow().clone()
    }

    pub fn status(&self) -> Option<Rc<Status>> {
        self.imp().status.borrow().clone()
    }

    pub fn toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }

    /// Runs a call to the service, showing any error as a toast.
    pub fn spawn<F, T>(&self, call: impl FnOnce(Client) -> F + 'static)
    where
        F: Future<Output = crate::client::Result<T>> + 'static,
    {
        let Some(client) = self.client() else {
            self.toast("Not connected to the Permafrost service");
            return;
        };
        let win = self.downgrade();
        glib::spawn_future_local(async move {
            if let Err(message) = call(client).await
                && let Some(win) = win.upgrade()
            {
                win.toast(&message);
            }
        });
    }

    fn connect_service(&self) {
        let imp = self.imp();
        imp.stack.set_visible_child_name("loading");
        let win = self.downgrade();
        glib::spawn_future_local(async move {
            let connected = Client::connect().await;
            let Some(win) = win.upgrade() else { return };
            match connected {
                Ok(client) => {
                    win.imp().client.replace(Some(client.clone()));
                    win.listen(&client);
                    match client.status().await {
                        Ok(status) => {
                            win.apply_status(status);
                            win.show_section(Section::Freeze);
                        }
                        Err(e) => win.toast(&e),
                    }
                }
                Err(e) => {
                    eprintln!("couldn't reach permafrostd: {e}");
                    win.imp().stack.set_visible_child_name("offline");
                    win.imp().content_page.set_title("Permafrost");
                }
            }
        });
    }

    /// Follows the service's signals for as long as the window lives.
    fn listen(&self, client: &Client) {
        let win = self.downgrade();
        let proxy = client.proxy().clone();
        glib::spawn_future_local(async move {
            let Ok(mut changes) = proxy.receive_status_changed().await else { return };
            while let Some(signal) = changes.next().await {
                let Some(win) = win.upgrade() else { return };
                if let Ok(status) = signal.args().map_err(|e| e.to_string()).and_then(|a| {
                    serde_json::from_str::<Status>(a.status()).map_err(|e| e.to_string())
                }) {
                    win.apply_status(status);
                }
            }
        });

        let win = self.downgrade();
        let proxy = client.proxy().clone();
        glib::spawn_future_local(async move {
            let Ok(mut blocked) = proxy.receive_app_blocked().await else { return };
            while let Some(signal) = blocked.next().await {
                let Some(win) = win.upgrade() else { return };
                if let Ok(args) = signal.args() {
                    win.notify_app_blocked(args.app_id(), *args.until());
                }
            }
        });
    }

    fn apply_status(&self, status: Status) {
        let imp = self.imp();
        let status = Rc::new(status);
        imp.status.replace(Some(status.clone()));
        if imp.freeze_suffix.borrow().is_none() {
            self.build_sidebar();
        }

        imp.freeze_page.set_status(&status);
        imp.schedules_page.set_status(&status);
        imp.filters_page.set_status(&status);
        imp.settings_page.set_status(&status);
        imp.history_page.set_status(&status);

        let section = imp.section.borrow().clone();
        match section {
            Section::List(id) => match status.state.list(&id) {
                Some(list) => {
                    imp.list_page.set_list(list, &status);
                    imp.content_page.set_title(&list.name);
                }
                None => self.show_section(Section::Filters),
            },
            Section::Freeze => self.update_freeze_title(&status),
            Section::Schedules | Section::Filters | Section::History | Section::Settings => {}
        }

        match status.locked_until {
            Some(until) => {
                imp.banner.set_title(&format!(
                    "Frozen until {} — locked filters can only get stricter",
                    format::clock(until)
                ));
                imp.banner.set_revealed(true);
            }
            None => imp.banner.set_revealed(false),
        }

        self.update_background(&status);
        self.tick();
    }

    fn update_freeze_title(&self, status: &Status) {
        let frozen = status.state.freeze.as_ref().is_some_and(|f| f.ends_at > status.now);
        self.imp().content_page.set_title(if frozen { "Frozen" } else { "Freeze" });
    }

    /// Keeps the app alive in the background while anything is being blocked,
    /// or a schedule is about to start, so its notifications still arrive.
    fn update_background(&self, status: &Status) {
        let imp = self.imp();
        let active = !status.active_lists.is_empty();
        self.set_hold(active || !upcoming_schedules(status, HOLD_BEFORE_SCHEDULE).is_empty());

        let message = match (&status.state.freeze, status.locked_until) {
            (Some(f), _) if f.ends_at > status.now => format!("Frozen until {}", format::clock(f.ends_at)),
            (_, Some(until)) => format!("Blocking until {}", format::clock(until)),
            _ if active => "Blocking on schedule".to_owned(),
            _ => String::new(),
        };
        if *imp.background_message.borrow() != message && imp.background_requested.get() {
            imp.background_message.replace(message.clone());
            glib::spawn_future_local(async move { crate::background::set_status(&message).await });
        }
    }

    fn set_hold(&self, hold: bool) {
        let imp = self.imp();
        self.set_hide_on_close(hold);
        if hold {
            if imp.hold.borrow().is_none()
                && let Some(app) = self.application()
            {
                imp.hold.replace(Some(app.hold()));
            }
            if !imp.background_requested.replace(true) {
                glib::spawn_future_local(crate::background::request());
            }
        } else {
            imp.hold.replace(None);
        }
    }

    /// "Work Hours starts in 5 minutes", once per run of each schedule.
    fn remind_schedules(&self, status: &Status) {
        let Some(app) = self.application() else { return };
        let today = chrono::Local::now().date_naive();
        for (schedule, minutes) in upcoming_schedules(status, REMIND_BEFORE_SCHEDULE) {
            let key = format!("{}-{today}-{}", schedule.id, schedule.start);
            if !self.imp().reminded.borrow_mut().insert(key) {
                continue;
            }
            let names: Vec<&str> =
                schedule.lists.iter().filter_map(|id| status.state.list(id)).map(|l| l.name.as_str()).collect();
            let when = if minutes <= 1 { "in a minute".to_owned() } else { format!("in {minutes} minutes") };
            let notification = gio::Notification::new(&format!("{} starts {when}", schedule.name));
            let until = if schedule.all_day {
                "for the rest of the day".to_owned()
            } else {
                format!("until {}", format::minutes_of_day(schedule.end))
            };
            notification.set_body(Some(&format!("{} will be blocked {until}.", names.join(", "))));
            notification.set_icon(&gio::ThemedIcon::new(APP_SYMBOLIC));
            app.send_notification(Some(&format!("schedule-{}", schedule.id)), &notification);
        }
    }

    /// "Break time" and "Back to work" notifications for freezes with breaks.
    fn announce_breaks(&self, status: &Status, now: i64) {
        let imp = self.imp();
        let phase = status.state.freeze.as_ref().filter(|f| f.breaks.is_some() && f.ends_at > now).map(|f| f.phase(now));
        let on_break = phase.map(|p| matches!(p, Phase::OnBreak { .. }));
        let was = imp.on_break.replace(on_break);
        let (Some(app), Some(was), Some(on_break), Some(phase)) = (self.application(), was, on_break, phase) else {
            return;
        };
        if was == on_break {
            return;
        }
        let notification = match phase {
            Phase::OnBreak { until, long, .. } => {
                let n = gio::Notification::new(if long { "Long break" } else { "Break time" });
                n.set_body(Some(&format!("Blocks are lifted until {}.", format::clock(until))));
                n
            }
            Phase::Working { round, .. } => {
                let n = gio::Notification::new(&format!("Round {round}: back to focus"));
                n.set_body(Some("Blocks are back on."));
                n
            }
        };
        notification.set_icon(&gio::ThemedIcon::new(APP_SYMBOLIC));
        app.send_notification(Some("freeze-break"), &notification);
    }

    fn notify_app_blocked(&self, app_id: &str, until: i64) {
        let Some(app) = self.application() else { return };
        let name = gio_unix::DesktopAppInfo::new(&format!("{app_id}.desktop"))
            .map(|info| info.display_name().to_string())
            .unwrap_or_else(|| app_id.to_owned());
        let notification = gio::Notification::new(&format!("{name} is frozen"));
        notification.set_body(Some(&format!("It can open again after {}.", format::clock(until))));
        notification.set_icon(&gio::ThemedIcon::new(APP_SYMBOLIC));
        app.send_notification(Some(&format!("blocked-{app_id}")), &notification);
    }

    /// Updates countdowns once a second.
    fn tick(&self) {
        let Some(status) = self.status() else { return };
        let now = chrono::Utc::now().timestamp();
        let imp = self.imp();
        imp.freeze_page.tick(now);
        self.remind_schedules(&status);
        self.announce_breaks(&status, now);
        if status.active_lists.is_empty() {
            self.set_hold(!upcoming_schedules(&status, HOLD_BEFORE_SCHEDULE).is_empty());
        }
        if let Some(label) = imp.freeze_suffix.borrow().as_ref() {
            match status.state.freeze.as_ref().filter(|f| f.ends_at > now) {
                Some(f) => {
                    label.set_label(&format::countdown(f.ends_at - now));
                    label.set_visible(true);
                }
                None => label.set_visible(false),
            }
        }
    }

    fn build_sidebar(&self) {
        let imp = self.imp();
        imp.rebuilding.set(true);
        let freeze_suffix = gtk::Label::builder().css_classes(["dimmed", "numeric"]).visible(false).build();
        let section = adw::SidebarSection::new();
        section.append(adw::SidebarItem::builder().title("Freeze").icon_name(APP_SYMBOLIC).suffix(&freeze_suffix).build());
        section.append(adw::SidebarItem::builder().title("Schedules").icon_name("alarm-symbolic").build());
        section.append(
            adw::SidebarItem::builder()
                .title("Filters")
                .icon_name("preferences-system-parental-controls-symbolic")
                .build(),
        );
        section.append(adw::SidebarItem::builder().title("History").icon_name("document-open-recent-symbolic").build());
        section.append(adw::SidebarItem::builder().title("Settings").icon_name("emblem-system-symbolic").build());
        imp.sidebar.append(section);
        imp.freeze_suffix.replace(Some(freeze_suffix));
        let current = imp.section.borrow().clone();
        imp.sidebar.set_selected(index_of(&current));
        imp.rebuilding.set(false);
    }

    fn on_sidebar_selected(&self, index: u32, activated: bool) {
        let imp = self.imp();
        if imp.rebuilding.get() || imp.client.borrow().is_none() {
            return;
        }
        let section = match index {
            0 => Section::Freeze,
            1 => Section::Schedules,
            2 => Section::Filters,
            3 => Section::History,
            _ => Section::Settings,
        };
        self.show_section(section);
        if activated {
            imp.split_view.set_show_content(true);
        }
    }

    /// In a narrow window, moves from the sidebar to the content.
    pub fn show_content(&self) {
        self.imp().split_view.set_show_content(true);
    }

    pub fn show_section(&self, section: Section) {
        let imp = self.imp();
        let Some(status) = self.status() else { return };
        match &section {
            Section::Freeze => {
                imp.stack.set_visible_child_name("freeze");
                self.update_freeze_title(&status);
            }
            Section::Schedules => {
                imp.stack.set_visible_child_name("schedules");
                imp.content_page.set_title("Schedules");
            }
            Section::Filters => {
                imp.stack.set_visible_child_name("filters");
                imp.content_page.set_title("Filters");
            }
            Section::History => {
                imp.stack.set_visible_child_name("history");
                imp.content_page.set_title("History");
            }
            Section::Settings => {
                imp.stack.set_visible_child_name("settings");
                imp.content_page.set_title("Settings");
            }
            Section::List(id) => {
                let Some(list) = status.state.list(id) else { return };
                imp.list_page.set_list(list, &status);
                imp.stack.set_visible_child_name("list");
                imp.content_page.set_title(&list.name);
            }
        }
        let viewing_list = matches!(section, Section::List(_));
        imp.list_menu_button.set_visible(viewing_list);
        imp.back_button.set_visible(viewing_list);
        let index = index_of(&section);
        if imp.sidebar.selected() != index {
            imp.rebuilding.set(true);
            imp.sidebar.set_selected(index);
            imp.rebuilding.set(false);
        }
        imp.section.replace(section);
    }

    fn selected_list(&self) -> Option<BlockList> {
        let Section::List(id) = self.imp().section.borrow().clone() else { return None };
        self.status()?.state.list(&id).cloned()
    }

    /// Asks for a name, then creates a new list or renames `existing`.
    fn prompt_list_name(&self, existing: Option<BlockList>) {
        let renaming = existing.is_some();
        let entry = gtk::Entry::builder()
            .text(existing.as_ref().map(|l| l.name.as_str()).unwrap_or_default())
            .placeholder_text("Name")
            .activates_default(true)
            .build();
        let dialog = adw::AlertDialog::builder()
            .heading(if renaming { "Rename Filter" } else { "New Custom Filter" })
            .extra_child(&entry)
            .default_response("save")
            .close_response("cancel")
            .build();
        dialog.add_responses(&[("cancel", "_Cancel"), ("save", if renaming { "_Rename" } else { "_Create" })]);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_response_enabled("save", renaming);
        entry.connect_changed(glib::clone!(
            #[weak]
            dialog,
            move |entry| dialog.set_response_enabled("save", !entry.text().trim().is_empty())
        ));

        dialog.connect_response(
            Some("save"),
            glib::clone!(
                #[weak(rename_to = win)]
                self,
                #[weak]
                entry,
                move |_, _| {
                    let mut list = existing.clone().unwrap_or_default();
                    list.name = entry.text().trim().to_owned();
                    let weak = win.downgrade();
                    win.spawn(move |client| async move {
                        let id = client.save_list(&list).await?;
                        // The status signal may arrive after this returns; refresh to select the list.
                        let status = client.status().await?;
                        if let Some(win) = weak.upgrade() {
                            win.apply_status(status);
                            win.show_section(Section::List(id));
                        }
                        Ok(())
                    });
                }
            ),
        );
        dialog.present(Some(self));
        entry.grab_focus();
    }

    fn confirm_delete_list(&self) {
        let Some(list) = self.selected_list() else { return };
        let dialog = adw::AlertDialog::builder()
            .heading(format!("Delete “{}”?", list.name))
            .body("Schedules that use it will stop blocking these sites and apps.")
            .default_response("cancel")
            .close_response("cancel")
            .build();
        dialog.add_responses(&[("cancel", "_Cancel"), ("delete", "_Delete")]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.connect_response(
            Some("delete"),
            glib::clone!(
                #[weak(rename_to = win)]
                self,
                move |_, _| {
                    let id = list.id.clone();
                    win.spawn(move |client| async move { client.delete_list(&id).await });
                    win.show_section(Section::Filters);
                }
            ),
        );
        dialog.present(Some(self));
    }
}
