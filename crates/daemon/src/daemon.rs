//! The service's state and the rules for changing it.

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use nix::time::{ClockId, clock_gettime};
use permafrost_common::model::{BlockList, Freeze, Schedule, State, Status};
use permafrost_common::domain;

use crate::apps::AppEnforcer;
use crate::browsers;
use crate::community::Community;
use crate::hosts;
use crate::safesearch::SafeSearch;

const MAX_FREEZE: u64 = 7 * 24 * 60 * 60;

#[derive(Debug)]
pub enum Error {
    /// Not allowed right now, usually because something is frozen.
    Denied(String),
    Invalid(String),
    Failed(anyhow::Error),
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Self::Failed(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Options {
    pub state_file: PathBuf,
    pub hosts_file: PathBuf,
    /// Log what would be enforced instead of doing it.
    pub dry_run: bool,
}

/// What happened during a tick that clients should hear about.
#[derive(Default)]
pub struct TickReport {
    pub status: Option<String>,
    pub closed_apps: Vec<String>,
    pub locked_until: i64,
}

pub struct Daemon {
    state: State,
    options: Options,
    apps: AppEnforcer,
    safe_search: SafeSearch,
    community: Community,
    /// What the hosts file was last written from, to skip rewriting it every tick.
    hosts_key: Option<HostsKey>,
    /// Whether browser DNS-over-HTTPS policies are currently in place.
    doh_blocked: Option<bool>,
    last_active: BTreeSet<String>,
    /// Wall clock and boot clock at the last tick, to notice clock changes.
    last_clocks: Option<(i64, Duration)>,
}

type HostsKey = (Vec<BlockList>, u64, Vec<(IpAddr, String)>);

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn boottime() -> Duration {
    clock_gettime(ClockId::CLOCK_BOOTTIME).map(Duration::from).unwrap_or_default()
}

impl Daemon {
    pub fn load(options: Options) -> anyhow::Result<Self> {
        let mut state: State = match std::fs::read_to_string(&options.state_file) {
            Ok(json) => serde_json::from_str(&json).with_context(|| format!("reading {}", options.state_file.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(e).context("reading state"),
        };
        let seeded = state.seed_presets();
        let daemon = Self {
            state,
            apps: AppEnforcer::default(),
            safe_search: SafeSearch::default(),
            community: Community::new(options.state_file.with_file_name("community")),
            hosts_key: None,
            doh_blocked: None,
            last_active: BTreeSet::new(),
            last_clocks: None,
            options,
        };
        if seeded {
            daemon.save()?;
        }
        Ok(daemon)
    }

    pub fn status(&self) -> Status {
        self.status_at(now())
    }

    fn status_at(&self, now: i64) -> Status {
        let mut status = self.state.status(now);
        status.community_sizes = self.community.sizes();
        status
    }

    fn wanted_community(&self) -> BTreeSet<String> {
        self.state.lists.iter().flat_map(|l| l.community.iter().cloned()).collect()
    }

    pub fn status_json(&self) -> String {
        serde_json::to_string(&self.status()).expect("status serializes")
    }

    pub fn save_list(&mut self, json: &str) -> Result<String> {
        let mut list: BlockList = serde_json::from_str(json).map_err(|e| Error::Invalid(e.to_string()))?;
        list.name = list.name.trim().to_owned();
        if list.name.is_empty() {
            return Err(Error::Invalid("A block list needs a name".into()));
        }
        let mut sites = Vec::new();
        for site in &list.sites {
            let site = domain::normalize(site).ok_or_else(|| Error::Invalid(format!("“{site}” isn’t a website address")))?;
            if !sites.contains(&site) {
                sites.push(site);
            }
        }
        list.sites = sites;
        list.apps = list.apps.iter().map(|a| a.trim().trim_end_matches(".desktop").to_owned()).filter(|a| !a.is_empty()).collect();
        list.apps.dedup();

        self.status().check_list_update(&list).map_err(Error::Denied)?;
        if list.id.is_empty() {
            list.id = new_id(&list.name);
        }
        let id = list.id.clone();
        match self.state.lists.iter_mut().find(|l| l.id == id) {
            Some(existing) => *existing = list,
            None => self.state.lists.push(list),
        }
        self.commit()?;
        Ok(id)
    }

    pub fn delete_list(&mut self, id: &str) -> Result<()> {
        self.status().check_list_delete(id).map_err(Error::Denied)?;
        self.state.lists.retain(|l| l.id != id);
        for schedule in &mut self.state.schedules {
            schedule.lists.retain(|l| l != id);
        }
        self.commit()
    }

    pub fn save_schedule(&mut self, json: &str) -> Result<String> {
        let mut schedule: Schedule = serde_json::from_str(json).map_err(|e| Error::Invalid(e.to_string()))?;
        schedule.name = schedule.name.trim().to_owned();
        if schedule.name.is_empty() {
            return Err(Error::Invalid("A schedule needs a name".into()));
        }
        if schedule.start >= 24 * 60 || schedule.end >= 24 * 60 {
            return Err(Error::Invalid("Times must be within a day".into()));
        }
        schedule.lists.retain(|id| self.state.list(id).is_some());
        if !schedule.id.is_empty() {
            self.status().check_schedule_change(&schedule.id).map_err(Error::Denied)?;
        } else {
            schedule.id = new_id(&schedule.name);
        }
        let id = schedule.id.clone();
        match self.state.schedules.iter_mut().find(|s| s.id == id) {
            Some(existing) => *existing = schedule,
            None => self.state.schedules.push(schedule),
        }
        self.commit()?;
        Ok(id)
    }

    pub fn delete_schedule(&mut self, id: &str) -> Result<()> {
        self.status().check_schedule_change(id).map_err(Error::Denied)?;
        self.state.schedules.retain(|s| s.id != id);
        self.commit()
    }

    pub fn start_freeze(&mut self, lists: Vec<String>, seconds: u64, locked: bool) -> Result<()> {
        let now = now();
        if self.state.freeze.as_ref().is_some_and(|f| f.ends_at > now) {
            return Err(Error::Denied("Already frozen; add time instead".into()));
        }
        if !(60..=MAX_FREEZE).contains(&seconds) {
            return Err(Error::Invalid("A freeze lasts between a minute and a week".into()));
        }
        let lists: Vec<String> = lists.into_iter().filter(|id| self.state.list(id).is_some()).collect();
        if lists.is_empty() {
            return Err(Error::Invalid("Choose at least one block list".into()));
        }
        self.state.freeze = Some(Freeze { lists, started_at: now, ends_at: now + seconds as i64, locked });
        self.commit()
    }

    pub fn add_time(&mut self, seconds: u64) -> Result<()> {
        let now = now();
        let freeze = self.state.freeze.as_mut().filter(|f| f.ends_at > now).ok_or_else(|| Error::Denied("Nothing is frozen".into()))?;
        let remaining = (freeze.ends_at - now) as u64;
        if remaining + seconds > MAX_FREEZE {
            return Err(Error::Invalid("A freeze lasts at most a week".into()));
        }
        freeze.ends_at += seconds as i64;
        self.commit()
    }

    pub fn stop_freeze(&mut self) -> Result<()> {
        self.status().can_stop_freeze().map_err(Error::Denied)?;
        self.state.freeze = None;
        self.commit()
    }

    /// Runs every couple of seconds: tracks the clock, ends finished freezes
    /// and keeps blocks applied.
    pub fn tick(&mut self) -> TickReport {
        let (wall, boot) = (now(), boottime());
        let mut changed = false;
        if let Some((last_wall, last_boot)) = self.last_clocks {
            let real = boot.saturating_sub(last_boot).as_secs() as i64;
            let jump = (wall - last_wall) - real;
            // Moving the clock shouldn't end a freeze early (or late).
            if jump.abs() > 5
                && let Some(freeze) = &mut self.state.freeze
            {
                tracing::warn!("system clock moved by {jump}s; keeping the freeze’s remaining time");
                freeze.ends_at += jump;
                changed = true;
            }
        }
        self.last_clocks = Some((wall, boot));
        changed |= self.state.expire(wall);
        let wanted = self.wanted_community();
        changed |= self.community.refresh(&wanted);

        let status = self.status_at(wall);
        let closed_apps = self.enforce(&status);
        changed |= status.active_lists != self.last_active;
        self.last_active = status.active_lists.clone();
        if changed && let Err(e) = self.save() {
            tracing::error!("couldn't save state: {e:#}");
        }
        TickReport {
            status: changed.then(|| serde_json::to_string(&status).expect("status serializes")),
            closed_apps,
            locked_until: status.locked_until.or(status.state.freeze.as_ref().map(|f| f.ends_at)).unwrap_or(wall),
        }
    }

    /// Called on shutdown. Locked blocks stay in place while the service is stopped.
    pub fn shutdown(&mut self) {
        if self.status().locked_lists.is_empty() {
            self.write_hosts(String::new());
            self.hosts_key = None;
            browsers::set_doh_blocked(false, self.options.dry_run);
        }
    }

    fn enforce(&mut self, status: &Status) -> Vec<String> {
        let lists: Vec<BlockList> = status.active_lists.iter().filter_map(|id| self.state.list(id)).cloned().collect();
        let apps: BTreeSet<String> = lists.iter().flat_map(|l| l.apps.iter().cloned()).collect();
        let safe_search = lists.iter().any(|l| l.safe_search);
        let redirects = if safe_search { self.safe_search.redirects().to_vec() } else { Vec::new() };

        let key = (lists, self.community.generation(), redirects);
        if self.hosts_key.as_ref() != Some(&key) {
            let (lists, _, redirects) = &key;
            let sites: BTreeSet<String> =
                lists.iter().flat_map(|l| l.sites.iter().flat_map(|s| domain::expand(s))).collect();
            let community: BTreeSet<&str> = lists
                .iter()
                .flat_map(|l| l.community.iter())
                .flat_map(|id| self.community.hosts(id))
                .map(String::as_str)
                .filter(|host| !sites.contains(*host))
                .collect();
            let blocking = !block_is_empty(&sites, &community);
            let block = hosts::render(&sites, &community, redirects);
            if self.write_hosts(block) {
                self.hosts_key = Some(key);
            }
            if self.doh_blocked != Some(blocking) {
                browsers::set_doh_blocked(blocking, self.options.dry_run);
                self.doh_blocked = Some(blocking);
            }
        }
        self.apps.enforce(&apps, self.options.dry_run)
    }

    /// Returns whether the block is now in place.
    fn write_hosts(&mut self, block: String) -> bool {
        if self.options.dry_run {
            tracing::info!("would write {} hosts lines", block.lines().count());
        } else if let Err(e) = hosts::write(&self.options.hosts_file, &block) {
            tracing::error!("couldn't update {}: {e}", self.options.hosts_file.display());
            return false;
        }
        true
    }

    fn commit(&mut self) -> Result<()> {
        self.save()?;
        let wanted = self.wanted_community();
        self.community.refresh(&wanted);
        let status = self.status();
        self.last_active = status.active_lists.clone();
        self.enforce(&status);
        Ok(())
    }

    fn save(&self) -> anyhow::Result<()> {
        let path = &self.options.state_file;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.state)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

fn block_is_empty(sites: &BTreeSet<String>, community: &BTreeSet<&str>) -> bool {
    sites.is_empty() && community.is_empty()
}

/// A short, readable, unique id like `work-hours-k3j9`.
fn new_id(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("-");
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos();
    format!("{}-{:x}", if slug.is_empty() { "item" } else { &slug }, nanos & 0xfffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daemon(dir: &std::path::Path) -> Daemon {
        Daemon::load(Options {
            state_file: dir.join("state.json"),
            hosts_file: dir.join("hosts"),
            dry_run: false,
        })
        .unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("permafrost-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hosts"), "127.0.0.1 localhost\n").unwrap();
        dir
    }

    #[test]
    fn freeze_writes_hosts_and_locks() {
        let dir = temp_dir("freeze");
        let mut d = daemon(&dir);
        d.start_freeze(vec!["social".into()], 3600, true).unwrap();
        let hosts = std::fs::read_to_string(dir.join("hosts")).unwrap();
        assert!(hosts.starts_with("127.0.0.1 localhost\n"));
        assert!(hosts.contains("0.0.0.0 www.reddit.com"));
        assert!(matches!(d.stop_freeze(), Err(Error::Denied(_))));

        let mut social = d.state.list("social").unwrap().clone();
        social.sites.push("https://news.ycombinator.com/".into());
        d.save_list(&serde_json::to_string(&social).unwrap()).unwrap();
        assert!(d.state.list("social").unwrap().sites.contains(&"news.ycombinator.com".to_owned()));
        social.sites.clear();
        assert!(matches!(d.save_list(&serde_json::to_string(&social).unwrap()), Err(Error::Denied(_))));

        // Reloading keeps the freeze.
        let d = daemon(&dir);
        assert!(d.status().active_lists.contains("social"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unlocked_freeze_can_stop() {
        let dir = temp_dir("unlocked");
        let mut d = daemon(&dir);
        d.start_freeze(vec!["video".into()], 600, false).unwrap();
        d.stop_freeze().unwrap();
        let hosts = std::fs::read_to_string(dir.join("hosts")).unwrap();
        assert_eq!(hosts, "127.0.0.1 localhost\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ids_are_readable() {
        assert!(new_id("Work Hours!").starts_with("work-hours-"));
        assert!(new_id("☃").starts_with("item-"));
    }
}
