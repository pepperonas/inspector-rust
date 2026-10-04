//! When a task runs. Pure: the scheduler asks "when next?" and "does this
//! event concern you?", and both answers are computed here.

use chrono::{Datelike, Duration, LocalResult, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

pub const MIN_INTERVAL_MIN: u32 = 1;
/// One week — longer intervals belong in a schedule.
pub const MAX_INTERVAL_MIN: u32 = 7 * 24 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// Every `minutes` minutes, counted from the previous run.
    Interval { minutes: u32 },
    /// At `hour:minute` local time on the given weekdays (1 = Monday … 7 = Sunday).
    Schedule { weekdays: Vec<u8>, hour: u8, minute: u8 },
    /// When files appear or change directly inside `path`.
    Folder { path: String },
    /// When Inspector Rust starts.
    AppStart,
    /// When the computer wakes from sleep.
    Wake,
    /// Only by hand ("Run now").
    Manual,
}

/// Something that happened, matched against event triggers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    AppStart,
    Wake,
    /// Paths that changed inside a watched folder (the folder as configured).
    Folder { folder: String, paths: Vec<String> },
}

impl Trigger {
    /// The value for `IR_TASK_TRIGGER` — the contract stated in the prompt.
    pub fn kind(&self) -> &'static str {
        match self {
            Trigger::Interval { .. } => "interval",
            Trigger::Schedule { .. } => "schedule",
            Trigger::Folder { .. } => "folder",
            Trigger::AppStart => "app_start",
            Trigger::Wake => "wake",
            Trigger::Manual => "manual",
        }
    }

    /// Bring hand-edited or out-of-range values into range.
    pub fn normalized(self) -> Trigger {
        match self {
            Trigger::Interval { minutes } => Trigger::Interval {
                minutes: minutes.clamp(MIN_INTERVAL_MIN, MAX_INTERVAL_MIN),
            },
            Trigger::Schedule { weekdays, hour, minute } => {
                let mut days: Vec<u8> = weekdays.into_iter().filter(|d| (1..=7).contains(d)).collect();
                days.sort_unstable();
                days.dedup();
                if days.is_empty() {
                    days = (1..=7).collect();
                }
                Trigger::Schedule { weekdays: days, hour: hour.min(23), minute: minute.min(59) }
            }
            Trigger::Folder { path } => Trigger::Folder { path: path.trim().to_string() },
            other => other,
        }
    }

    /// Timed triggers: the next moment strictly after `after_ms` (epoch ms).
    /// `None` for event triggers.
    pub fn next_after<Tz: TimeZone>(&self, after_ms: i64, tz: &Tz) -> Option<i64> {
        match self {
            Trigger::Interval { minutes } => Some(after_ms + i64::from((*minutes).max(1)) * 60_000),
            Trigger::Schedule { weekdays, hour, minute } => next_schedule(after_ms, weekdays, *hour, *minute, tz),
            _ => None,
        }
    }

    /// Does `ev` start this task? Returns the changed paths for a folder event.
    pub fn matches(&self, ev: &Event) -> Option<Vec<String>> {
        match (self, ev) {
            (Trigger::AppStart, Event::AppStart) | (Trigger::Wake, Event::Wake) => Some(Vec::new()),
            (Trigger::Folder { path }, Event::Folder { folder, paths }) if same_folder(path, folder) => {
                Some(paths.clone())
            }
            _ => None,
        }
    }
}

fn same_folder(a: &str, b: &str) -> bool {
    a.trim_end_matches(['/', '\\']) == b.trim_end_matches(['/', '\\'])
}

/// The next `hour:minute` on one of `weekdays` strictly after `after_ms`, in `tz`.
/// A time that falls into a spring-forward gap runs at the first moment after
/// it; one that occurs twice in autumn runs once (the earlier).
fn next_schedule<Tz: TimeZone>(after_ms: i64, weekdays: &[u8], hour: u8, minute: u8, tz: &Tz) -> Option<i64> {
    let after = tz.timestamp_millis_opt(after_ms).single()?;
    let time = NaiveTime::from_hms_opt(u32::from(hour.min(23)), u32::from(minute.min(59)), 0)?;
    let days: Vec<u8> = if weekdays.is_empty() { (1..=7).collect() } else { weekdays.to_vec() };
    // 8 days covers "today, but the time has passed" for every weekday set.
    for offset in 0..=8 {
        let date = after.date_naive() + Duration::days(offset);
        if !days.contains(&(date.weekday().number_from_monday() as u8)) {
            continue;
        }
        let naive = date.and_time(time);
        let at = match tz.from_local_datetime(&naive) {
            LocalResult::Single(t) => t,
            LocalResult::Ambiguous(early, _) => early,
            // Spring-forward gap: the wall-clock time doesn't exist that day.
            LocalResult::None => match tz.from_local_datetime(&(naive + Duration::hours(1))) {
                LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => t,
                LocalResult::None => continue,
            },
        };
        let ms = at.timestamp_millis();
        if ms > after_ms {
            return Some(ms);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, NaiveDate, Utc};

    fn utc_ms(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, 0).unwrap().and_utc().timestamp_millis()
    }

    #[test]
    fn intervals_count_from_the_previous_run_and_are_clamped() {
        let t = Trigger::Interval { minutes: 15 };
        assert_eq!(t.next_after(1_000, &Utc), Some(1_000 + 15 * 60_000));
        assert_eq!(Trigger::Interval { minutes: 0 }.normalized(), Trigger::Interval { minutes: 1 });
        assert_eq!(
            Trigger::Interval { minutes: 99_999 }.normalized(),
            Trigger::Interval { minutes: MAX_INTERVAL_MIN }
        );
    }

    #[test]
    fn a_schedule_later_today_runs_today_an_earlier_one_on_the_next_matching_day() {
        // 2026-10-05 is a Monday.
        let mon_0800 = utc_ms(2026, 10, 5, 8, 0);
        let t = Trigger::Schedule { weekdays: vec![1, 3], hour: 8, minute: 30 };
        assert_eq!(t.next_after(mon_0800, &Utc), Some(utc_ms(2026, 10, 5, 8, 30)));
        // At 8:30 exactly it must not run again the same minute → Wednesday.
        assert_eq!(t.next_after(utc_ms(2026, 10, 5, 8, 30), &Utc), Some(utc_ms(2026, 10, 7, 8, 30)));
        // Wednesday after the time → next Monday.
        assert_eq!(t.next_after(utc_ms(2026, 10, 7, 9, 0), &Utc), Some(utc_ms(2026, 10, 12, 8, 30)));
    }

    #[test]
    fn a_schedule_uses_the_local_wall_clock() {
        let berlin_summer = FixedOffset::east_opt(2 * 3600).unwrap();
        // 06:00 UTC = 08:00 local; the task at 08:30 local is 06:30 UTC.
        let t = Trigger::Schedule { weekdays: vec![], hour: 8, minute: 30 };
        assert_eq!(t.next_after(utc_ms(2026, 10, 5, 6, 0), &berlin_summer), Some(utc_ms(2026, 10, 5, 6, 30)));
    }

    #[test]
    fn schedules_are_normalised() {
        let t = Trigger::Schedule { weekdays: vec![9, 3, 3, 0, 1], hour: 30, minute: 99 }.normalized();
        assert_eq!(t, Trigger::Schedule { weekdays: vec![1, 3], hour: 23, minute: 59 });
        let all = Trigger::Schedule { weekdays: vec![], hour: 1, minute: 1 }.normalized();
        assert_eq!(all, Trigger::Schedule { weekdays: (1..=7).collect(), hour: 1, minute: 1 });
    }

    #[test]
    fn event_triggers_have_no_time_and_match_only_their_event() {
        assert_eq!(Trigger::Wake.next_after(0, &Utc), None);
        assert_eq!(Trigger::Wake.matches(&Event::Wake), Some(vec![]));
        assert_eq!(Trigger::Wake.matches(&Event::AppStart), None);
        assert_eq!(Trigger::AppStart.matches(&Event::AppStart), Some(vec![]));
        assert_eq!(Trigger::Manual.matches(&Event::AppStart), None);
        let f = Trigger::Folder { path: "/Users/x/Downloads/".into() };
        let ev = Event::Folder { folder: "/Users/x/Downloads".into(), paths: vec!["/Users/x/Downloads/a.pdf".into()] };
        assert_eq!(f.matches(&ev), Some(vec!["/Users/x/Downloads/a.pdf".into()]));
        let other = Event::Folder { folder: "/Users/x/Desktop".into(), paths: vec![] };
        assert_eq!(f.matches(&other), None);
    }

    #[test]
    fn triggers_serialise_with_a_type_tag() {
        let v = serde_json::to_value(Trigger::Schedule { weekdays: vec![1], hour: 8, minute: 0 }).unwrap();
        assert_eq!(v["type"], "schedule");
        let back: Trigger = serde_json::from_str(r#"{"type":"app_start"}"#).unwrap();
        assert_eq!(back, Trigger::AppStart);
        assert_eq!(Trigger::AppStart.kind(), "app_start");
    }
}
