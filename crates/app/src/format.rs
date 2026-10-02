//! Turning times and durations into text.

use chrono::{Local, TimeZone};
use permafrost_common::model::{BlockList, Schedule, Status};

/// `47:12`, or `1:02:03` past an hour.
pub fn countdown(seconds: i64) -> String {
    let s = seconds.max(0);
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

/// `15:30` today, `Tue 15:30` otherwise.
pub fn clock(timestamp: i64) -> String {
    let Some(at) = Local.timestamp_opt(timestamp, 0).single() else {
        return String::new();
    };
    let tomorrow = Local::now().date_naive() + chrono::Duration::days(1);
    if at.date_naive() == tomorrow && at.format("%H:%M").to_string() == "00:00" {
        "midnight".to_owned()
    } else if at.date_naive() == Local::now().date_naive() {
        at.format("%H:%M").to_string()
    } else {
        at.format("%a %H:%M").to_string()
    }
}

/// `2 h 10 min`, `45 min`, `0 min`.
pub fn duration(seconds: u64) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

pub fn minutes_of_day(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

pub fn days(days: &[bool; 7]) -> String {
    const NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    match days {
        [true, true, true, true, true, true, true] => "Every day".into(),
        [true, true, true, true, true, false, false] => "Weekdays".into(),
        [false, false, false, false, false, true, true] => "Weekends".into(),
        _ if days.iter().all(|d| !d) => "No days".into(),
        _ => NAMES.iter().zip(days).filter(|(_, on)| **on).map(|(n, _)| *n).collect::<Vec<_>>().join(", "),
    }
}

pub fn schedule_summary(schedule: &Schedule, list_names: &[String]) -> String {
    let hours = if schedule.all_day {
        "All day".to_owned()
    } else {
        format!("{}–{}", minutes_of_day(schedule.start), minutes_of_day(schedule.end))
    };
    let mut parts = vec![days(&schedule.days), hours];
    if !list_names.is_empty() {
        parts.push(list_names.join(", "));
    }
    parts.join(" · ")
}

/// `76793` → `76,793`.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Sites a list blocks, counting its community lists once they're downloaded.
pub fn site_count(list: &BlockList, status: &Status) -> usize {
    list.sites.len() + list.community.iter().filter_map(|c| status.community_sizes.get(c)).sum::<usize>()
}

pub fn list_summary(list: &BlockList, status: &Status) -> String {
    let mut summary = counts(site_count(list, status), list.apps.len());
    if let Some(limit) = list.daily_limit {
        let used = usage_today(status, &list.id);
        summary.push_str(&format!(" · {} of {} today", duration(used), duration(u64::from(limit) * 60)));
    }
    summary
}

/// Seconds `list` has been in use today.
pub fn usage_today(status: &Status, list: &str) -> u64 {
    let today = permafrost_common::model::day_key(chrono::Local::now().date_naive());
    status.state.history.get(&today).and_then(|d| d.usage.get(list)).copied().unwrap_or(0)
}

fn counts(sites: usize, apps: usize) -> String {
    let plural = |n: usize, one: &str, many: &str| format!("{} {}", thousands(n), if n == 1 { one } else { many });
    match (sites, apps) {
        (0, 0) => "Empty".into(),
        (s, 0) => plural(s, "site", "sites"),
        (0, a) => plural(a, "app", "apps"),
        (s, a) => format!("{} · {}", plural(s, "site", "sites"), plural(a, "app", "apps")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(countdown(47 * 60 + 12), "47:12");
        assert_eq!(countdown(3723), "1:02:03");
        assert_eq!(days(&[true, false, true, false, true, false, false]), "Mon, Wed, Fri");
        assert_eq!(counts(5, 1), "5 sites · 1 app");
        assert_eq!(counts(76_828, 0), "76,828 sites");
        assert_eq!(thousands(999), "999");
        assert_eq!(duration(130 * 60), "2 h 10 min");
        assert_eq!(duration(45 * 60 + 59), "45 min");
        assert_eq!(duration(7200), "2 h");
    }
}
