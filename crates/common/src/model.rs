//! What Permafrost stores, and the rules for changing it while frozen.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockList {
    pub id: String,
    pub name: String,
    /// Normalized domains; common subdomains are blocked too.
    #[serde(default)]
    pub sites: Vec<String>,
    /// Desktop app IDs without the `.desktop` suffix, e.g. `com.valvesoftware.Steam`.
    #[serde(default)]
    pub apps: Vec<String>,
    /// Community blocklists (`community::SOURCES` ids) included in this list.
    #[serde(default)]
    pub community: Vec<String>,
    /// Minutes a day its sites and apps may be used before it's blocked until midnight.
    #[serde(default)]
    pub daily_limit: Option<u32>,
}

/// App-wide settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Force SafeSearch on Google, Bing, DuckDuckGo and YouTube, all the time.
    #[serde(default)]
    pub safe_search: bool,
    /// Whether new freezes can't be stopped before their timer ends.
    #[serde(default = "default_true")]
    pub lock_freezes: bool,
    /// Community lists from versions that turned them on globally; moved
    /// into block lists by `State::migrate_community`.
    #[serde(default, rename = "community", skip_serializing_if = "Vec::is_empty")]
    pub legacy_community: Vec<String>,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self { safe_search: false, lock_freezes: true, legacy_community: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Monday first.
    pub days: [bool; 7],
    /// Minutes after midnight. An end before the start runs overnight.
    pub start: u16,
    pub end: u16,
    pub lists: Vec<String>,
    /// Can't be turned off or edited while it's running.
    pub locked: bool,
    /// Runs the whole of each selected day; `start` and `end` are ignored.
    #[serde(default)]
    pub all_day: bool,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: true,
            days: [true, true, true, true, true, false, false],
            start: 9 * 60,
            end: 17 * 60,
            lists: Vec::new(),
            locked: false,
            all_day: false,
        }
    }
}

impl Schedule {
    /// The end of the run that covers `at`, if one does.
    pub fn active_until<Tz: TimeZone>(&self, at: &DateTime<Tz>) -> Option<DateTime<Tz>> {
        let day = at.weekday().num_days_from_monday() as usize;
        let midnight = at.clone() - chrono::Duration::seconds(i64::from(at.num_seconds_from_midnight()));
        if self.enabled && self.all_day {
            return self.days[day].then(|| midnight + chrono::Duration::days(1));
        }
        if !self.enabled || self.start == self.end {
            return None;
        }
        let yesterday = (day + 6) % 7;
        let minute = (at.hour() * 60 + at.minute()) as u16;
        let end_today = midnight.clone() + chrono::Duration::minutes(i64::from(self.end));

        if self.start < self.end {
            (self.days[day] && (self.start..self.end).contains(&minute)).then_some(end_today)
        } else if self.days[day] && minute >= self.start {
            Some(end_today + chrono::Duration::days(1))
        } else if self.days[yesterday] && minute < self.end {
            Some(end_today)
        } else {
            None
        }
    }
}

/// What happened on one day.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayStats {
    /// Time with anything blocked, from freezes or schedules.
    #[serde(default)]
    pub focus_seconds: u64,
    #[serde(default)]
    pub freezes: u32,
    #[serde(default)]
    pub apps_closed: u32,
    /// Seconds each block list with a daily limit was in use.
    #[serde(default)]
    pub usage: BTreeMap<String, u64>,
}

/// A day counts towards a streak with at least this much focus.
pub const STREAK_SECONDS: u64 = 15 * 60;

pub fn day_key(date: chrono::NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Days in a row with enough focus, up to `today`. Today not counting yet
/// doesn't break the streak.
pub fn streak(history: &BTreeMap<String, DayStats>, today: chrono::NaiveDate) -> u32 {
    let counts = |date| history.get(&day_key(date)).is_some_and(|d| d.focus_seconds >= STREAK_SECONDS);
    let mut day = if counts(today) { today } else { today - chrono::Duration::days(1) };
    let mut days = 0;
    while counts(day) {
        days += 1;
        day -= chrono::Duration::days(1);
    }
    days
}

/// Pomodoro cycles: focus for `work_minutes`, then a short break, with a
/// longer break after every `long_break_every` rounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breaks {
    pub work_minutes: u32,
    pub break_minutes: u32,
    #[serde(default)]
    pub long_break_minutes: u32,
    /// 0 for no long breaks.
    #[serde(default)]
    pub long_break_every: u32,
    /// How many focus rounds the freeze has; 0 if it's just a long freeze with breaks.
    #[serde(default)]
    pub rounds: u32,
}

impl Breaks {
    /// Minutes of break after focus round `round` (counting from 1).
    pub fn break_after(&self, round: u32) -> u32 {
        let long = self.long_break_every > 0 && self.long_break_minutes > 0 && round.is_multiple_of(self.long_break_every);
        if long { self.long_break_minutes } else { self.break_minutes }
    }

    /// How long `rounds` focus rounds take, with the breaks between them.
    pub fn total_seconds(&self, rounds: u32) -> u64 {
        let breaks: u64 = (1..rounds).map(|r| u64::from(self.break_after(r))).sum();
        (u64::from(self.work_minutes) * u64::from(rounds) + breaks) * 60
    }
}

/// Where a freeze with breaks is in its cycle. `round` counts from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Working { until: i64, round: u32 },
    OnBreak { until: i64, round: u32, long: bool },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Freeze {
    pub lists: Vec<String>,
    /// Unix seconds.
    pub started_at: i64,
    pub ends_at: i64,
    pub locked: bool,
    #[serde(default)]
    pub breaks: Option<Breaks>,
    /// Block everything except these lists' sites and apps.
    #[serde(default)]
    pub allow_only: bool,
}

impl Freeze {
    pub fn phase(&self, now: i64) -> Phase {
        let Some(breaks) = self.breaks.filter(|b| b.work_minutes > 0) else {
            return Phase::Working { until: self.ends_at, round: 1 };
        };
        let mut start = self.started_at;
        let mut round = 1;
        while start < self.ends_at {
            let work_end = start + i64::from(breaks.work_minutes) * 60;
            if now < work_end {
                return Phase::Working { until: work_end.min(self.ends_at), round };
            }
            let minutes = breaks.break_after(round);
            let break_end = work_end + i64::from(minutes) * 60;
            if now < break_end {
                let long = minutes != breaks.break_minutes;
                return Phase::OnBreak { until: break_end.min(self.ends_at), round, long };
            }
            start = break_end;
            round += 1;
        }
        Phase::Working { until: self.ends_at, round }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub lists: Vec<BlockList>,
    #[serde(default)]
    pub schedules: Vec<Schedule>,
    #[serde(default)]
    pub freeze: Option<Freeze>,
    #[serde(default, alias = "filters")]
    pub settings: Settings,
    /// Presets already added once, so deleting one doesn't bring it back.
    #[serde(default)]
    pub seeded_presets: Vec<String>,
    /// Whether preset lists have been given their community lists.
    #[serde(default)]
    pub community_seeded: bool,
    /// Per local day (`YYYY-MM-DD`), most recent 90 days.
    #[serde(default)]
    pub history: BTreeMap<String, DayStats>,
}

/// What's in effect right now, as the service sees it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub state: State,
    pub now: i64,
    /// Lists being enforced, from the freeze and running schedules.
    pub active_lists: BTreeSet<String>,
    /// Lists that may only get stricter until `locked_until`.
    pub locked_lists: BTreeSet<String>,
    pub locked_until: Option<i64>,
    /// Schedules running now, which can't be edited if they're locked.
    pub running_schedules: BTreeSet<String>,
    /// While an allow-only freeze is working: the lists that are still allowed.
    #[serde(default)]
    pub allow_lists: BTreeSet<String>,
    /// Lists whose daily limit is used up; they're blocked and locked until midnight.
    #[serde(default)]
    pub limit_reached: BTreeSet<String>,
    /// While a freeze is on a break: when the break ends.
    #[serde(default)]
    pub on_break_until: Option<i64>,
    /// How many sites each downloaded community blocklist has.
    #[serde(default)]
    pub community_sizes: BTreeMap<String, usize>,
}

impl State {
    pub fn list(&self, id: &str) -> Option<&BlockList> {
        self.lists.iter().find(|l| l.id == id)
    }

    pub fn schedule(&self, id: &str) -> Option<&Schedule> {
        self.schedules.iter().find(|s| s.id == id)
    }

    /// Adds presets this state hasn't seen yet as block lists. Returns whether anything changed.
    pub fn seed_presets(&mut self) -> bool {
        let mut changed = false;
        for preset in crate::presets::PRESETS {
            if self.seeded_presets.iter().any(|id| id == preset.id) {
                continue;
            }
            if self.list(preset.id).is_none() {
                self.lists.push(preset.to_list(preset.id.to_owned()));
            }
            self.seeded_presets.push(preset.id.to_owned());
            changed = true;
        }
        changed
    }

    /// Gives preset lists their community lists once, and moves community lists
    /// that older versions turned on globally into the list they belong to.
    pub fn migrate_community(&mut self) -> bool {
        use crate::community::Category;

        let mut additions: Vec<(&str, String)> = Vec::new();
        if !self.community_seeded {
            for preset in crate::presets::PRESETS {
                additions.extend(preset.community.iter().map(|c| (preset.id, (*c).to_owned())));
            }
        }
        for source in std::mem::take(&mut self.settings.legacy_community) {
            let Some(found) = crate::community::find(&source) else { continue };
            let list = match found.category {
                Category::Adult | Category::Protection => "adult",
                Category::Gambling => "gambling",
                Category::Distractions => "social",
                Category::Harmful => "harmful",
            };
            additions.push((list, source));
        }

        let mut changed = !self.community_seeded;
        for (list_id, source) in additions {
            if let Some(list) = self.lists.iter_mut().find(|l| l.id == list_id)
                && !list.community.contains(&source)
            {
                list.community.push(source);
                changed = true;
            }
        }
        changed |= !self.community_seeded;
        self.community_seeded = true;
        changed
    }

    /// Resets the preset filters to their original contents, bringing back
    /// deleted ones, and resets settings. Custom filters and schedules stay.
    pub fn restore_defaults(&mut self) {
        for preset in crate::presets::PRESETS {
            let fresh = preset.to_list(preset.id.to_owned());
            match self.lists.iter_mut().find(|l| l.id == preset.id) {
                Some(list) => *list = fresh,
                None => self.lists.push(fresh),
            }
            if !self.seeded_presets.iter().any(|id| id == preset.id) {
                self.seeded_presets.push(preset.id.to_owned());
            }
        }
        self.settings = Settings::default();
    }

    /// Ends an expired freeze. Returns whether anything changed.
    pub fn expire(&mut self, now: i64) -> bool {
        let expired = self.freeze.as_ref().is_some_and(|f| f.ends_at <= now);
        if expired {
            self.freeze = None;
        }
        expired
    }

    pub fn status(&self, now: i64) -> Status {
        let mut status = Status { state: self.clone(), now, ..Default::default() };
        let lock_until =|until: i64, lists: &[String], status: &mut Status| {
            status.locked_lists.extend(lists.iter().cloned());
            status.locked_until = Some(status.locked_until.map_or(until, |u| u.max(until)));
        };

        if let Some(freeze) = self.freeze.as_ref().filter(|f| f.ends_at > now) {
            // A break lifts the blocks, but the freeze stays locked.
            match freeze.phase(now) {
                Phase::Working { .. } if freeze.allow_only => status.allow_lists.extend(freeze.lists.iter().cloned()),
                Phase::Working { .. } => status.active_lists.extend(freeze.lists.iter().cloned()),
                Phase::OnBreak { until, .. } => status.on_break_until = Some(until),
            }
            if freeze.locked {
                lock_until(freeze.ends_at, &freeze.lists, &mut status);
            }
        }

        let local = Local.timestamp_opt(now, 0).single().unwrap_or_else(Local::now);
        for schedule in &self.schedules {
            if let Some(until) = schedule.active_until(&local) {
                status.running_schedules.insert(schedule.id.clone());
                status.active_lists.extend(schedule.lists.iter().cloned());
                if schedule.locked {
                    lock_until(until.timestamp(), &schedule.lists, &mut status);
                }
            }
        }

        let today = self.history.get(&day_key(local.date_naive()));
        let next_midnight = (local.date_naive() + chrono::Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .and_then(|m| m.and_local_timezone(Local).earliest())
            .map_or(now + 24 * 60 * 60, |m| m.timestamp());
        for list in &self.lists {
            let Some(limit) = list.daily_limit else { continue };
            let used = today.and_then(|d| d.usage.get(&list.id)).copied().unwrap_or(0);
            if used >= u64::from(limit) * 60 {
                status.limit_reached.insert(list.id.clone());
                status.active_lists.insert(list.id.clone());
                lock_until(next_midnight, std::slice::from_ref(&list.id), &mut status);
            }
        }

        status.active_lists.retain(|id| self.list(id).is_some());
        status
    }
}

impl Status {
    /// Checks that saving `new` doesn't loosen a locked list.
    pub fn check_list_update(&self, new: &BlockList) -> Result<(), String> {
        if !self.locked_lists.contains(&new.id) {
            return Ok(());
        }
        let Some(old) = self.state.list(&new.id) else {
            return Ok(());
        };
        // An allow list gets stricter by allowing less.
        if self.state.freeze.as_ref().is_some_and(|f| f.allow_only && f.lists.contains(&new.id)) {
            let added_site = new.sites.iter().any(|s| !old.sites.contains(s));
            let added_app = new.apps.iter().any(|a| !old.apps.contains(a));
            if added_site || added_app {
                return Err(format!("“{}” is frozen as an allow list: you can remove from it, but not add", old.name));
            }
            return Ok(());
        }
        let removed_site = old.sites.iter().any(|s| !new.sites.contains(s));
        let removed_app = old.apps.iter().any(|a| !new.apps.contains(a));
        let removed_source = old.community.iter().any(|c| !new.community.contains(c));
        let raised_limit = match (old.daily_limit, new.daily_limit) {
            (Some(old), Some(new)) => new > old,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if removed_site || removed_app || removed_source || raised_limit {
            return Err(format!("“{}” is frozen: you can add to it, but not remove from it", old.name));
        }
        Ok(())
    }

    /// While anything is locked, SafeSearch can be turned on but not off.
    pub fn check_settings_update(&self, new: &Settings) -> Result<(), String> {
        let loosened = self.state.settings.safe_search && !new.safe_search;
        if loosened && !self.locked_lists.is_empty() {
            return Err("SafeSearch can’t be turned off while something is frozen".into());
        }
        Ok(())
    }

    pub fn can_restore_defaults(&self) -> Result<(), String> {
        if !self.locked_lists.is_empty() {
            return Err("Defaults can’t be restored while something is frozen".into());
        }
        Ok(())
    }

    pub fn check_list_delete(&self, id: &str) -> Result<(), String> {
        if self.locked_lists.contains(id) {
            return Err("This list is frozen and can’t be deleted until the block ends".into());
        }
        Ok(())
    }

    pub fn check_schedule_change(&self, id: &str) -> Result<(), String> {
        let locked = self.state.schedule(id).is_some_and(|s| s.locked);
        if locked && self.running_schedules.contains(id) {
            return Err("This schedule is running and locked until it ends".into());
        }
        Ok(())
    }

    pub fn can_stop_freeze(&self) -> Result<(), String> {
        match &self.state.freeze {
            Some(f) if f.locked && f.ends_at > self.now => {
                Err("This freeze is locked until the timer ends".into())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, NaiveDate};

    fn at(day: u32, h: u32, m: u32) -> DateTime<FixedOffset> {
        // 2026-10-05 is a Monday.
        let naive = NaiveDate::from_ymd_opt(2026, 10, day).unwrap().and_hms_opt(h, m, 0).unwrap();
        FixedOffset::east_opt(0).unwrap().from_local_datetime(&naive).unwrap()
    }

    #[test]
    fn daytime_schedule() {
        let s = Schedule::default(); // weekdays 09:00–17:00
        assert_eq!(s.active_until(&at(5, 10, 0)), Some(at(5, 17, 0)));
        assert_eq!(s.active_until(&at(5, 17, 0)), None);
        assert_eq!(s.active_until(&at(10, 10, 0)), None, "Saturday is off");
    }

    #[test]
    fn overnight_schedule() {
        let s = Schedule { start: 22 * 60, end: 6 * 60, days: [true, false, false, false, false, false, false], ..Default::default() };
        assert_eq!(s.active_until(&at(5, 23, 0)), Some(at(6, 6, 0)), "Monday night");
        assert_eq!(s.active_until(&at(6, 5, 0)), Some(at(6, 6, 0)), "continues into Tuesday");
        assert_eq!(s.active_until(&at(6, 23, 0)), None, "Tuesday isn't selected");
    }

    #[test]
    fn all_day_schedule() {
        let s = Schedule { all_day: true, days: [true, false, false, false, false, false, true], ..Default::default() };
        assert_eq!(s.active_until(&at(5, 0, 0)), Some(at(6, 0, 0)), "all of Monday");
        assert_eq!(s.active_until(&at(5, 23, 59)), Some(at(6, 0, 0)));
        assert_eq!(s.active_until(&at(6, 12, 0)), None, "Tuesday is off");
    }

    #[test]
    fn streaks() {
        let day = |d| NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
        let focused = DayStats { focus_seconds: STREAK_SECONDS, ..Default::default() };
        let mut history = BTreeMap::new();
        for d in [1, 2, 3, 5] {
            history.insert(day_key(day(d)), focused.clone());
        }
        assert_eq!(streak(&history, day(3)), 3);
        assert_eq!(streak(&history, day(4)), 3, "today hasn't counted yet");
        assert_eq!(streak(&history, day(5)), 1);
        assert_eq!(streak(&history, day(7)), 0);
    }

    #[test]
    fn breaks_lift_blocks_but_keep_the_lock() {
        let mut state = frozen_state();
        let freeze = state.freeze.as_mut().unwrap();
        freeze.ends_at = 3 * 60 * 60;
        freeze.breaks = Some(Breaks { work_minutes: 25, break_minutes: 5, ..Default::default() });
        assert_eq!(freeze.phase(60), Phase::Working { until: 25 * 60, round: 1 });
        assert_eq!(freeze.phase(26 * 60), Phase::OnBreak { until: 30 * 60, round: 1, long: false });
        assert_eq!(freeze.phase(31 * 60), Phase::Working { until: 55 * 60, round: 2 });

        let status = state.status(26 * 60);
        assert!(status.active_lists.is_empty());
        assert_eq!(status.on_break_until, Some(30 * 60));
        assert!(status.locked_lists.contains("a"));
        assert!(status.can_stop_freeze().is_err());
    }

    #[test]
    fn pomodoro_long_breaks() {
        let breaks = Breaks { work_minutes: 25, break_minutes: 5, long_break_minutes: 15, long_break_every: 4, rounds: 4 };
        // Four rounds, with short breaks between them and no break at the end.
        assert_eq!(breaks.total_seconds(4), (4 * 25 + 3 * 5) * 60);
        assert_eq!(breaks.total_seconds(8), (8 * 25 + 6 * 5 + 15) * 60);
        let freeze = Freeze { lists: vec![], started_at: 0, ends_at: breaks.total_seconds(8) as i64, locked: true, breaks: Some(breaks), allow_only: false };
        let round_four_ends = (4 * 25 + 3 * 5) * 60;
        assert_eq!(freeze.phase(round_four_ends + 60), Phase::OnBreak { until: round_four_ends + 15 * 60, round: 4, long: true });
        assert!(matches!(freeze.phase(round_four_ends + 16 * 60), Phase::Working { round: 5, .. }));
    }

    #[test]
    fn daily_limits_block_until_midnight() {
        let today = Local::now().date_naive();
        let mut state = State {
            lists: vec![BlockList { id: "games".into(), name: "Games".into(), daily_limit: Some(30), ..Default::default() }],
            ..Default::default()
        };
        let now = Local::now().timestamp();
        let mut day = DayStats::default();
        day.usage.insert("games".into(), 29 * 60);
        state.history.insert(day_key(today), day);
        assert!(state.status(now).limit_reached.is_empty());

        state.history.get_mut(&day_key(today)).unwrap().usage.insert("games".into(), 30 * 60);
        let status = state.status(now);
        assert!(status.limit_reached.contains("games"));
        assert!(status.active_lists.contains("games"));
        let mut raised = state.lists[0].clone();
        raised.daily_limit = Some(60);
        assert!(status.check_list_update(&raised).is_err(), "can't raise a used-up limit");
    }

    #[test]
    fn allow_lists_only_shrink_while_locked() {
        let mut state = frozen_state();
        state.freeze.as_mut().unwrap().allow_only = true;
        let status = state.status(10);
        assert!(status.active_lists.is_empty(), "allow lists aren't blocked");
        assert!(status.allow_lists.contains("a"));
        let mut list = state.lists[0].clone();
        list.sites.push("reddit.com".into());
        assert!(status.check_list_update(&list).is_err());
        list.sites.clear();
        assert!(status.check_list_update(&list).is_ok());
    }

    fn frozen_state() -> State {
        State {
            lists: vec![BlockList { id: "a".into(), name: "Social".into(), sites: vec!["x.com".into()], ..Default::default() }],
            freeze: Some(Freeze { lists: vec!["a".into()], started_at: 0, ends_at: 100, locked: true, breaks: None, allow_only: false }),
            ..Default::default()
        }
    }

    #[test]
    fn locked_lists_only_get_stricter() {
        let status = frozen_state().status(10);
        let mut list = status.state.lists[0].clone();
        list.sites.push("reddit.com".into());
        assert!(status.check_list_update(&list).is_ok());
        list.sites.clear();
        assert!(status.check_list_update(&list).is_err());
        assert!(status.check_list_delete("a").is_err());
        assert!(status.can_stop_freeze().is_err());
    }

    #[test]
    fn seeds_presets_once() {
        let mut state = State::default();
        assert!(state.seed_presets());
        assert!(state.list("gambling").is_some() && state.list("adult").is_some());
        state.lists.retain(|l| l.id != "gambling");
        assert!(!state.seed_presets(), "a deleted preset stays deleted");
        assert!(state.list("gambling").is_none());
    }

    #[test]
    fn safe_search_only_gets_stricter_while_locked() {
        let mut state = frozen_state();
        state.settings.safe_search = true;
        assert!(state.status(10).check_settings_update(&Settings::default()).is_err());
        assert!(state.status(10).can_restore_defaults().is_err());
        assert!(state.status(100).check_settings_update(&Settings::default()).is_ok(), "unlocked again");
    }

    #[test]
    fn migrates_community_lists_into_presets() {
        // An install from before per-list community lists, with one turned on globally.
        let mut state = State::default();
        state.seed_presets();
        for list in &mut state.lists {
            list.community.clear();
        }
        state.community_seeded = false;
        state.settings.legacy_community = vec!["hagezi-gambling".into()];

        assert!(state.migrate_community());
        assert!(state.list("adult").unwrap().community.contains(&"stevenblack-porn".to_owned()));
        assert!(state.list("gambling").unwrap().community.contains(&"hagezi-gambling".to_owned()));
        assert!(state.settings.legacy_community.is_empty());
        assert!(!state.migrate_community(), "runs once");
    }

    #[test]
    fn restore_defaults_keeps_custom_filters() {
        let mut state = State::default();
        state.seed_presets();
        state.lists.retain(|l| l.id != "video");
        state.lists[0].sites.clear();
        state.lists.push(BlockList { id: "mine".into(), name: "Mine".into(), ..Default::default() });
        state.settings.safe_search = true;
        state.restore_defaults();
        assert!(state.list("video").is_some());
        assert!(!state.lists[0].sites.is_empty());
        assert!(state.list("mine").is_some());
        assert_eq!(state.settings, Settings::default());
    }

    #[test]
    fn reads_old_filters_key() {
        let state: State = serde_json::from_str(r#"{"filters":{"safe_search":true}}"#).unwrap();
        assert!(state.settings.safe_search);
        assert!(state.settings.lock_freezes);
    }

    #[test]
    fn expired_freeze_unlocks() {
        let mut state = frozen_state();
        assert!(state.expire(100));
        let status = state.status(100);
        assert!(status.active_lists.is_empty());
        assert!(status.can_stop_freeze().is_ok());
    }
}
