//! Turning times and durations into text.

use chrono::{Local, TimeZone};
use permafrost_common::model::Schedule;

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
    if at.date_naive() == Local::now().date_naive() {
        at.format("%H:%M").to_string()
    } else {
        at.format("%a %H:%M").to_string()
    }
}

pub fn minutes_of_day(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

pub fn freeze_button(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("Freeze for {m} Minutes"),
        (1, 0) => "Freeze for 1 Hour".into(),
        (h, 0) => format!("Freeze for {h} Hours"),
        (h, m) => format!("Freeze for {h} h {m} min"),
    }
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
    let mut parts = vec![
        days(&schedule.days),
        format!("{}–{}", minutes_of_day(schedule.start), minutes_of_day(schedule.end)),
    ];
    if !list_names.is_empty() {
        parts.push(list_names.join(", "));
    }
    parts.join(" · ")
}

pub fn list_summary(sites: usize, apps: usize) -> String {
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
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
        assert_eq!(freeze_button(25), "Freeze for 25 Minutes");
        assert_eq!(freeze_button(60), "Freeze for 1 Hour");
        assert_eq!(freeze_button(90), "Freeze for 1 h 30 min");
        assert_eq!(days(&[true, false, true, false, true, false, false]), "Mon, Wed, Fri");
        assert_eq!(list_summary(5, 1), "5 sites · 1 app");
    }
}
