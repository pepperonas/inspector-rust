//! Handover of consolidated slots to **bcsbook**, the tool that books hours
//! into Projektron BCS.
//!
//! bcsbook runs as a local GUI server on `127.0.0.1:4747` and stores one
//! override row per date. The transport is its existing API:
//!
//! - `GET  /api/suggestions?date=…` — what the day currently holds
//! - `POST /api/suggestions/save`   — write the day (header `x-bcsbook: 1`)
//!
//! **The save endpoint replaces the whole day** (`INSERT OR REPLACE` on the
//! date row). Pushing blindly would therefore wipe whatever else is already
//! there — rows derived from git commits, from bcsbook's own presence
//! tracking, and any manual correction the user made. So the push reads the
//! day first and merges: by default only slots that don't collide with an
//! existing entry are added, and the caller is told what was skipped. Replacing
//! the day is possible but has to be asked for explicitly.

use super::slots::Slot;
use serde::{Deserialize, Serialize};

/// A row in bcsbook's per-day override array (its `SavedSuggestion`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BcsRow {
    pub shortcut: String,
    pub from: String,
    pub to: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushResult {
    /// Rows actually written for this date.
    pub written: usize,
    /// Slots added on top of what was already there.
    pub added: usize,
    /// Slots skipped because they collided with an existing entry.
    pub skipped: usize,
    /// Slots without a project→shortcut mapping (never sent).
    pub unmapped: Vec<String>,
    pub base_url: String,
}

/// Local minutes-of-day for an epoch millisecond, in the machine's timezone.
fn local_minutes(ms: i64) -> i64 {
    use chrono::{Local, TimeZone, Timelike};
    match Local.timestamp_millis_opt(ms) {
        chrono::LocalResult::Single(dt) => dt.hour() as i64 * 60 + dt.minute() as i64,
        _ => 0,
    }
}

fn hhmm(min: i64) -> String {
    format!("{:02}:{:02}", (min / 60).clamp(0, 23), min % 60)
}

/// Convert slots into bcsbook rows, dropping any whose project has no mapping.
/// Returns the rows plus the labels that could not be mapped, so the UI can say
/// exactly which projects still need a shortcut. Pure — unit-tested.
pub fn slots_to_rows(slots: &[Slot], map: &[(String, String)]) -> (Vec<BcsRow>, Vec<String>) {
    let mut rows = Vec::new();
    let mut unmapped: Vec<String> = Vec::new();
    for s in slots {
        let Some(project) = s.project.as_deref().filter(|p| !p.is_empty()) else {
            if !unmapped.iter().any(|u| u == &s.label) {
                unmapped.push(s.label.clone());
            }
            continue;
        };
        let shortcut = map
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(project))
            .map(|(_, sc)| sc.clone());
        let Some(shortcut) = shortcut else {
            if !unmapped.iter().any(|u| u == project) {
                unmapped.push(project.to_string());
            }
            continue;
        };
        let from = local_minutes(s.start_ms);
        let to = local_minutes(s.end_ms);
        if to <= from {
            continue; // a slot crossing midnight is not a bookable day row
        }
        rows.push(BcsRow {
            shortcut,
            from: hhmm(from),
            to: hhmm(to),
            description: s.description.clone(),
            label: Some(project.to_string()),
            // Quarter hours — the slots are already snapped to the grid, so
            // this agrees with the window instead of drifting from it.
            hours: Some(((to - from) as f64 / 60.0 * 4.0).round() / 4.0),
        });
    }
    (rows, unmapped)
}

fn to_min(hhmm: &str) -> Option<i64> {
    let (h, m) = hhmm.split_once(':')?;
    Some(h.trim().parse::<i64>().ok()? * 60 + m.trim().parse::<i64>().ok()?)
}

fn overlaps(a: &BcsRow, b: &BcsRow) -> bool {
    let (Some(af), Some(at), Some(bf), Some(bt)) =
        (to_min(&a.from), to_min(&a.to), to_min(&b.from), to_min(&b.to))
    else {
        return false;
    };
    af < bt && bf < at
}

/// Merge new rows into the day already stored in bcsbook.
///
/// `replace` throws the existing day away. Otherwise existing rows are kept
/// verbatim and only non-colliding new rows are appended — BCS rejects
/// overlapping bookings outright, and silently discarding somebody's manual
/// correction would be worse than skipping an import row. Returns the merged
/// day plus how many rows were added and skipped. Pure — unit-tested.
pub fn merge_rows(existing: &[BcsRow], incoming: &[BcsRow], replace: bool) -> (Vec<BcsRow>, usize, usize) {
    if replace {
        return (incoming.to_vec(), incoming.len(), 0);
    }
    let mut out = existing.to_vec();
    let (mut added, mut skipped) = (0usize, 0usize);
    for row in incoming {
        if out.iter().any(|e| overlaps(e, row)) {
            skipped += 1;
            continue;
        }
        out.push(row.clone());
        added += 1;
    }
    out.sort_by_key(|r| to_min(&r.from).unwrap_or(0));
    (out, added, skipped)
}

#[derive(Deserialize)]
struct SuggestionsResponse {
    #[serde(default)]
    suggestions: Vec<BcsRow>,
}

/// Read the day currently stored in bcsbook. A missing/unreachable server is an
/// error the caller surfaces; an empty day is `Ok(vec![])`.
pub fn fetch_day(base_url: &str, date: &str) -> Result<Vec<BcsRow>, String> {
    let url = format!("{}/api/suggestions?date={date}", base_url.trim_end_matches('/'));
    let resp = ureq::get(&url)
        .set("x-bcsbook", "1")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| format!("bcsbook nicht erreichbar ({base_url}): {e}"))?;
    let txt = resp.into_string().map_err(|e| e.to_string())?;
    let parsed: SuggestionsResponse =
        serde_json::from_str(&txt).map_err(|e| format!("unerwartete Antwort von bcsbook: {e}"))?;
    Ok(parsed.suggestions)
}

/// Write the day back to bcsbook.
pub fn save_day(base_url: &str, date: &str, rows: &[BcsRow]) -> Result<(), String> {
    let url = format!("{}/api/suggestions/save", base_url.trim_end_matches('/'));
    let body = serde_json::json!({ "date": date, "suggestions": rows });
    ureq::post(&url)
        .set("x-bcsbook", "1")
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send_string(&body.to_string())
        .map_err(|e| format!("Speichern in bcsbook fehlgeschlagen: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(shortcut: &str, from: &str, to: &str) -> BcsRow {
        BcsRow {
            shortcut: shortcut.into(),
            from: from.into(),
            to: to.into(),
            description: String::new(),
            label: None,
            hours: None,
        }
    }

    fn slot(project: Option<&str>, start_ms: i64, end_ms: i64) -> Slot {
        Slot {
            start_ms,
            end_ms,
            project: project.map(str::to_string),
            label: project.unwrap_or("Mail").to_string(),
            description: "desc".into(),
            origin: super::super::slots::Origin::Tagged,
            apps: vec![],
            event_ids: vec![],
            active_s: (end_ms - start_ms) / 1000,
            span_s: (end_ms - start_ms) / 1000,
            confidence: 1.0,
        }
    }

    /// Local midnight today, so the fixtures produce stable HH:MM regardless of
    /// the machine's timezone.
    fn local_midnight_ms() -> i64 {
        use chrono::{Local, TimeZone};
        let today = Local::now().date_naive();
        Local
            .from_local_datetime(&today.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn slots_map_to_rows_with_agreeing_hours() {
        let base = local_midnight_ms();
        let s = slot(Some("alpha"), base + 9 * 3_600_000, base + 11 * 3_600_000 + 45 * 60_000);
        let map = vec![("alpha".to_string(), "AL".to_string())];
        let (rows, unmapped) = slots_to_rows(&[s], &map);
        assert!(unmapped.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].shortcut, "AL");
        assert_eq!(rows[0].from, "09:00");
        assert_eq!(rows[0].to, "11:45");
        assert_eq!(rows[0].hours, Some(2.75), "hours must match the window");
    }

    #[test]
    fn the_mapping_is_case_insensitive() {
        let base = local_midnight_ms();
        let s = slot(Some("Kiez-Finder"), base + 3_600_000, base + 2 * 3_600_000);
        let map = vec![("kiez-finder".to_string(), "KF".to_string())];
        assert_eq!(slots_to_rows(&[s], &map).0[0].shortcut, "KF");
    }

    #[test]
    fn unmapped_and_projectless_slots_are_reported_not_sent() {
        let base = local_midnight_ms();
        let a = slot(Some("beta"), base + 3_600_000, base + 2 * 3_600_000);
        let b = slot(None, base + 3 * 3_600_000, base + 4 * 3_600_000);
        let (rows, unmapped) = slots_to_rows(&[a, b], &[("alpha".to_string(), "AL".to_string())]);
        assert!(rows.is_empty());
        assert_eq!(unmapped, vec!["beta".to_string(), "Mail".to_string()]);
    }

    #[test]
    fn merging_keeps_existing_rows_and_appends_non_colliding_ones() {
        let existing = vec![row("GIT", "08:00", "09:00")];
        let incoming = vec![row("AL", "09:00", "11:00")];
        let (merged, added, skipped) = merge_rows(&existing, &incoming, false);
        assert_eq!(merged.len(), 2);
        assert_eq!((added, skipped), (1, 0));
        assert_eq!(merged[0].shortcut, "GIT", "sorted by start time");
    }

    #[test]
    fn a_colliding_row_is_skipped_rather_than_overwriting_existing_work() {
        let existing = vec![row("MANUAL", "09:00", "12:00")];
        let incoming = vec![row("AL", "10:00", "11:00")];
        let (merged, added, skipped) = merge_rows(&existing, &incoming, false);
        assert_eq!(merged, existing, "the day is untouched");
        assert_eq!((added, skipped), (0, 1));
    }

    #[test]
    fn touching_rows_do_not_count_as_colliding() {
        let existing = vec![row("GIT", "08:00", "09:00")];
        let incoming = vec![row("AL", "09:00", "10:00")];
        assert_eq!(merge_rows(&existing, &incoming, false).1, 1);
    }

    #[test]
    fn replace_discards_the_existing_day_but_only_when_asked() {
        let existing = vec![row("MANUAL", "09:00", "12:00")];
        let incoming = vec![row("AL", "10:00", "11:00")];
        let (merged, added, skipped) = merge_rows(&existing, &incoming, true);
        assert_eq!(merged, incoming);
        assert_eq!((added, skipped), (1, 0));
    }

    #[test]
    fn pushing_the_same_slots_twice_adds_nothing_the_second_time() {
        let incoming = vec![row("AL", "09:00", "11:00")];
        let (day1, _, _) = merge_rows(&[], &incoming, false);
        let (day2, added, skipped) = merge_rows(&day1, &incoming, false);
        assert_eq!(day2.len(), 1, "no duplicate");
        assert_eq!((added, skipped), (0, 1));
    }

    #[test]
    fn malformed_times_never_claim_an_overlap() {
        let existing = vec![row("X", "not-a-time", "also-not")];
        let incoming = vec![row("AL", "09:00", "10:00")];
        assert_eq!(merge_rows(&existing, &incoming, false).1, 1);
    }
}
