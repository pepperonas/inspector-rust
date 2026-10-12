//! Spotify history export: JSON, CSV and Markdown. Pure — the caller passes
//! the rows and how to write a timestamp, so tests pin the exact output.
//!
//! Timestamps are ISO 8601 / RFC 3339 with the offset that applied at that
//! moment (`2026-10-11T02:24:12+02:00`) — unambiguous and still readable as
//! local time. Durations are whole seconds.

use super::store::PlayRow;
#[cfg(test)]
use chrono::FixedOffset;
use chrono::{Local, TimeZone};

/// Local time with its offset at that instant (DST-correct per row).
pub fn iso_local(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, false))
        .unwrap_or_default()
}

/// Fixed offset (tests, reproducible output).
#[cfg(test)]
pub fn iso_fixed(ms: i64, offset: FixedOffset) -> String {
    offset
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, false))
        .unwrap_or_default()
}

fn secs(ms: u64) -> u64 {
    (ms + 500) / 1000
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Csv,
    Markdown,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "json" => Some(Self::Json),
            "csv" => Some(Self::Csv),
            "md" | "markdown" => Some(Self::Markdown),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Csv => "csv",
            Self::Markdown => "md",
        }
    }
}

pub fn render(rows: &[PlayRow], format: Format, time: &dyn Fn(i64) -> String, exported_at: i64) -> String {
    match format {
        Format::Json => json(rows, time, exported_at),
        Format::Csv => csv(rows, time),
        Format::Markdown => markdown(rows, time, exported_at),
    }
}

/// Structured data, not the preview: one object per play.
pub fn json(rows: &[PlayRow], time: &dyn Fn(i64) -> String, exported_at: i64) -> String {
    let plays: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "started_at": time(r.started_at),
                "ended_at": time(r.ended_at),
                "track": r.track,
                "artist": r.artist,
                "album": r.album,
                "track_uri": r.track_uri,
                "duration_seconds": r.duration_ms.map(secs),
                "played_seconds": secs(r.played_ms),
                "status": r.status.as_str(),
            })
        })
        .collect();
    let doc = serde_json::json!({
        "source": "Inspector Rust — Spotify history",
        "version": 1,
        "exported_at": time(exported_at),
        "count": rows.len(),
        "plays": plays,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n"
}

pub const CSV_HEADER: &str =
    "started_at,ended_at,artist,track,album,track_uri,duration_seconds,played_seconds,status";

/// Quote a CSV field (RFC 4180). A cell that starts with `= + - @` would run
/// as a formula in a spreadsheet — a track name is untrusted text, so such
/// cells get a leading `'`.
fn cell(s: &str) -> String {
    let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

pub fn csv(rows: &[PlayRow], time: &dyn Fn(i64) -> String) -> String {
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for r in rows {
        let fields = [
            time(r.started_at),
            time(r.ended_at),
            cell(&r.artist),
            cell(&r.track),
            cell(&r.album),
            cell(&r.track_uri),
            r.duration_ms.map(|d| secs(d).to_string()).unwrap_or_default(),
            secs(r.played_ms).to_string(),
            r.status.as_str().to_string(),
        ];
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

fn clock(ms: u64) -> String {
    let s = secs(ms);
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Markdown table cell: pipes and line breaks would break the table.
fn md(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// A readable log, grouped by day (the date part of `time`).
pub fn markdown(rows: &[PlayRow], time: &dyn Fn(i64) -> String, exported_at: i64) -> String {
    let mut out = format!(
        "# {}\n\n{}\n",
        crate::i18n::t("spx.title"),
        crate::i18n::tf(
            "spx.exported",
            &[("date", &time(exported_at)), ("n", &rows.len().to_string())],
        ),
    );
    let header = format!(
        "| {} | {} | {} | {} | {} | {} | {} |\n|---|---|---|---|---|---|---|\n",
        crate::i18n::t("ts.time"),
        crate::i18n::t("spx.track"),
        crate::i18n::t("spx.artist"),
        crate::i18n::t("spx.album"),
        crate::i18n::t("spx.played"),
        crate::i18n::t("spx.length"),
        crate::i18n::t("spx.status"),
    );
    let mut day = String::new();
    for r in rows {
        let stamp = time(r.started_at);
        let (date, rest) = stamp.split_once('T').unwrap_or((stamp.as_str(), ""));
        if date != day {
            day = date.to_string();
            out.push_str(&format!("\n## {day}\n\n{header}"));
        }
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            rest.get(..5).unwrap_or(rest),
            md(&r.track),
            md(&r.artist),
            md(&r.album),
            clock(r.played_ms),
            r.duration_ms.map(clock).unwrap_or_default(),
            r.status.as_str(),
        ));
    }
    out
}

/// `stem.ext`, or the first free `stem N.ext` — an export never overwrites.
pub fn free_name(stem: &str, ext: &str, exists: impl Fn(&str) -> bool) -> String {
    let first = format!("{stem}.{ext}");
    if !exists(&first) {
        return first;
    }
    (2..10_000)
        .map(|n| format!("{stem} {n}.{ext}"))
        .find(|name| !exists(name))
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spotify::PlayStatus;

    fn row(id: i64, start: i64, track: &str, artist: &str, played: u64, status: PlayStatus) -> PlayRow {
        PlayRow {
            id,
            started_at: start,
            ended_at: start + played as i64,
            track_uri: format!("spotify:track:{id}"),
            track: track.into(),
            artist: artist.into(),
            album: "Album".into(),
            duration_ms: Some(272_000),
            played_ms: played,
            status,
            artwork_url: None,
            open: false,
        }
    }

    // 2026-10-11 00:24:12 UTC
    const T: i64 = 1_791_678_252_000;

    fn cest(ms: i64) -> String {
        iso_fixed(ms, FixedOffset::east_opt(2 * 3600).unwrap())
    }

    #[test]
    fn json_is_structured_with_iso_timestamps() {
        let rows = [row(1, T, "Wicked Games", "Parra for Cuva", 261_000, PlayStatus::Listened)];
        let v: serde_json::Value = serde_json::from_str(&json(&rows, &cest, T)).unwrap();
        assert_eq!(v["count"], 1);
        let p = &v["plays"][0];
        assert_eq!(p["started_at"], "2026-10-11T02:24:12+02:00");
        assert_eq!(p["ended_at"], "2026-10-11T02:28:33+02:00");
        assert_eq!(p["track"], "Wicked Games");
        assert_eq!(p["duration_seconds"], 272);
        assert_eq!(p["played_seconds"], 261);
        assert_eq!(p["status"], "listened");
        assert_eq!(p["track_uri"], "spotify:track:1");
        assert!(p.get("open").is_none() && p.get("id").is_none(), "no internals");
    }

    #[test]
    fn csv_has_the_header_quotes_and_defuses_formulas() {
        let mut r = row(2, T, "Hey, \"you\"", "=cmd", 18_000, PlayStatus::Skipped);
        r.duration_ms = None;
        let out = csv(&[r], &cest);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], CSV_HEADER);
        assert_eq!(
            lines[1],
            "2026-10-11T02:24:12+02:00,2026-10-11T02:24:30+02:00,'=cmd,\"Hey, \"\"you\"\"\",Album,spotify:track:2,,18,skipped"
        );
    }

    #[test]
    fn markdown_groups_by_day_and_escapes_pipes() {
        let rows = [
            row(1, T, "A|B", "X", 60_000, PlayStatus::Partial),
            row(2, T + 86_400_000, "C", "Y", 3_725_000, PlayStatus::Listened),
        ];
        let out = markdown(&rows, &cest, T);
        assert!(out.contains("## 2026-10-11"));
        assert!(out.contains("## 2026-10-12"));
        assert!(out.contains("| 02:24 | A\\|B | X | Album | 1:00 | 4:32 | partial |"));
        assert!(out.contains("| 1:02:05 |"));
    }

    #[test]
    fn formats_and_free_names() {
        assert_eq!(Format::parse(""), Some(Format::Json));
        assert_eq!(Format::parse("CSV"), Some(Format::Csv));
        assert_eq!(Format::parse("markdown"), Some(Format::Markdown));
        assert_eq!(Format::parse("xml"), None);
        let taken = ["h.json", "h 2.json"];
        assert_eq!(free_name("h", "json", |n| taken.contains(&n)), "h 3.json");
        assert_eq!(free_name("h", "csv", |n| taken.contains(&n)), "h.csv");
    }
}
