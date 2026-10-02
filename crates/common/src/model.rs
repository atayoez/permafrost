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
}

/// Always-on filters, set in one place rather than per block list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filters {
    /// Force SafeSearch on Google, Bing, DuckDuckGo and YouTube.
    #[serde(default)]
    pub safe_search: bool,
    /// Community blocklists (`community::SOURCES` ids) to block.
    #[serde(default)]
    pub community: Vec<String>,
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
        }
    }
}

impl Schedule {
    /// The end of the run that covers `at`, if one does.
    pub fn active_until<Tz: TimeZone>(&self, at: &DateTime<Tz>) -> Option<DateTime<Tz>> {
        if !self.enabled || self.start == self.end {
            return None;
        }
        let day = at.weekday().num_days_from_monday() as usize;
        let yesterday = (day + 6) % 7;
        let minute = (at.hour() * 60 + at.minute()) as u16;
        let midnight = at.clone() - chrono::Duration::seconds(i64::from(at.num_seconds_from_midnight()));
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Freeze {
    pub lists: Vec<String>,
    /// Unix seconds.
    pub started_at: i64,
    pub ends_at: i64,
    pub locked: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub lists: Vec<BlockList>,
    #[serde(default)]
    pub schedules: Vec<Schedule>,
    #[serde(default)]
    pub freeze: Option<Freeze>,
    #[serde(default)]
    pub filters: Filters,
    /// Presets already added once, so deleting one doesn't bring it back.
    #[serde(default)]
    pub seeded_presets: Vec<String>,
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
            status.active_lists.extend(freeze.lists.iter().cloned());
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
        let removed_site = old.sites.iter().any(|s| !new.sites.contains(s));
        let removed_app = old.apps.iter().any(|a| !new.apps.contains(a));
        if removed_site || removed_app {
            return Err(format!("“{}” is frozen: you can add to it, but not remove from it", old.name));
        }
        Ok(())
    }

    /// While anything is locked, filters can be turned on but not off.
    pub fn check_filters_update(&self, new: &Filters) -> Result<(), String> {
        let old = &self.state.filters;
        let loosened = (old.safe_search && !new.safe_search) || old.community.iter().any(|c| !new.community.contains(c));
        if loosened && !self.locked_lists.is_empty() {
            return Err("Filters can’t be turned off while something is frozen".into());
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

    fn frozen_state() -> State {
        State {
            lists: vec![BlockList { id: "a".into(), name: "Social".into(), sites: vec!["x.com".into()], ..Default::default() }],
            freeze: Some(Freeze { lists: vec!["a".into()], started_at: 0, ends_at: 100, locked: true }),
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
    fn filters_only_get_stricter_while_locked() {
        let mut state = frozen_state();
        state.filters = Filters { safe_search: true, community: vec!["stevenblack-porn".into()] };
        let status = state.status(10);
        assert!(status.check_filters_update(&Filters { safe_search: true, community: vec![] }).is_err());
        assert!(status.check_filters_update(&Filters { safe_search: false, community: vec!["stevenblack-porn".into()] }).is_err());
        let stricter = Filters { safe_search: true, community: vec!["stevenblack-porn".into(), "hagezi-nsfw".into()] };
        assert!(status.check_filters_update(&stricter).is_ok());
        assert!(state.status(100).check_filters_update(&Filters::default()).is_ok(), "unlocked again");
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
