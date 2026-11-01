//! Pure, platform-independent core of the recorder: sizing, bitrate, the video
//! timestamp gate, the pause clock, cursor visibility/type, project directory
//! names and the disk-space verdict. Ported from OpenScreen
//! (`CaptureSizing.swift`, `VideoTimestampGate.swift`, the cursor helper).

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

// ---------- sizing ----------

fn even_floor(v: i64) -> u32 {
    let c = v.max(2);
    (c - c % 2) as u32
}

/// Output buffer size for the stream: content (points) x scale, one factor for
/// both axes when capping, even and >= 2. Unusable input falls back to the ceiling.
pub fn capture_output_size(
    content_w_pt: f64,
    content_h_pt: f64,
    point_pixel_scale: f64,
    max_w: u32,
    max_h: u32,
) -> (u32, u32) {
    let cw = even_floor(max_w as i64);
    let ch = even_floor(max_h as i64);
    let scale = if point_pixel_scale.is_finite() && point_pixel_scale > 0.0 {
        point_pixel_scale
    } else {
        1.0
    };
    let pw = if content_w_pt.is_finite() { content_w_pt * scale } else { 0.0 };
    let ph = if content_h_pt.is_finite() { content_h_pt * scale } else { 0.0 };
    if pw < 1.0 || ph < 1.0 {
        return (cw, ch);
    }
    let fit = (cw as f64 / pw).min(ch as f64 / ph).min(1.0);
    (
        even_floor((pw * fit).round() as i64),
        even_floor((ph * fit).round() as i64),
    )
}

/// 0.1 bit per pixel per frame, clamped to 2..60 Mbit/s.
pub fn default_video_bitrate(w: u32, h: u32, fps: u32) -> u32 {
    let pps = w.max(1) as f64 * h.max(1) as f64 * fps.max(1) as f64;
    ((pps * 0.1) as i64).clamp(2_000_000, 60_000_000) as u32
}

/// H.264 keyframe interval in frames: one second.
pub fn key_frame_interval(fps: u32) -> u32 {
    fps.max(1)
}

// ---------- video timestamp gate ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Admit,
    Invalid,
    NotAfterPrevious,
}

/// Keeps presentation times strictly increasing (AVAssetWriter fails otherwise).
#[derive(Debug, Default)]
pub struct VideoTimestampGate {
    last: Option<i64>,
    pub rejected: u32,
}

impl VideoTimestampGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts a refusal, records nothing else.
    pub fn check(&mut self, pts_ticks: Option<i64>) -> GateVerdict {
        let Some(pts) = pts_ticks else {
            self.rejected += 1;
            return GateVerdict::Invalid;
        };
        if let Some(last) = self.last {
            if pts <= last {
                self.rejected += 1;
                return GateVerdict::NotAfterPrevious;
            }
        }
        GateVerdict::Admit
    }

    /// Call just before handing the frame to the writer.
    pub fn record(&mut self, pts_ticks: i64) {
        self.last = Some(pts_ticks);
    }
}

// ---------- pause clock ----------

#[derive(Debug, Default)]
pub struct PauseClock {
    paused_at: Option<i64>,
    offset: i64,
    /// Completed pauses as host-time intervals `[pause, resume)`.
    intervals: Vec<(i64, i64)>,
}

impl PauseClock {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn pause(&mut self, now_ns: i64) {
        if self.paused_at.is_none() {
            self.paused_at = Some(now_ns);
        }
    }
    pub fn resume(&mut self, now_ns: i64) {
        if let Some(p) = self.paused_at.take() {
            self.offset += (now_ns - p).max(0);
            if now_ns > p {
                self.intervals.push((p, now_ns));
            }
        }
    }
    pub fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }
    pub fn offset_ns(&self) -> i64 {
        self.offset
    }
    /// Retiming offset for something CAPTURED at host time `pts_ns` (see
    /// [`offset_at`]); `None` = captured during a pause.
    pub fn offset_at(&self, pts_ns: i64) -> Option<i64> {
        offset_at(&self.intervals, self.paused_at, pts_ns)
    }
    /// Host time minus total paused time; frozen while paused.
    pub fn timeline_ns(&self, now_ns: i64) -> i64 {
        self.paused_at.unwrap_or(now_ns) - self.offset
    }
}

/// Decide on CAPTURE time, not delivery time: a sample captured at host
/// time `pts_ns` inside a pause (`[pause, resume)`, or at/after the start of
/// the still-open one) is dropped (`None`); otherwise its offset is the sum
/// of the pauses that ENDED at or before it — so a pre-pause sample that is
/// delivered late (during or after the pause) still gets offset 0.
pub fn offset_at(closed: &[(i64, i64)], open_since: Option<i64>, pts_ns: i64) -> Option<i64> {
    if open_since.is_some_and(|p| pts_ns >= p) {
        return None;
    }
    let mut off = 0;
    for &(p, r) in closed {
        if pts_ns >= p && pts_ns < r {
            return None;
        }
        if r <= pts_ns {
            off += r - p;
        }
    }
    Some(off)
}

/// Shared pause clock (capture, microphone and cursor sampler all use one).
#[derive(Debug, Default)]
pub struct PauseShared(Mutex<PauseClock>);

impl PauseShared {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn pause(&self, now_ns: i64) {
        self.0.lock().pause(now_ns)
    }
    pub fn resume(&self, now_ns: i64) {
        self.0.lock().resume(now_ns)
    }
    pub fn is_paused(&self) -> bool {
        self.0.lock().is_paused()
    }
    pub fn offset_ns(&self) -> i64 {
        self.0.lock().offset_ns()
    }
    pub fn timeline_ns(&self, now_ns: i64) -> i64 {
        self.0.lock().timeline_ns(now_ns)
    }
    pub fn offset_at(&self, pts_ns: i64) -> Option<i64> {
        self.0.lock().offset_at(pts_ns)
    }
}

// ---------- cursor ----------

#[derive(Debug, Default)]
pub struct CursorVisibility {
    outside_run: u8,
}

impl CursorVisibility {
    pub fn new() -> Self {
        Self::default()
    }
    /// Visible until the 3rd outside sample in a row; any inside sample restores it.
    pub fn observe(&mut self, inside: bool) -> bool {
        if inside {
            self.outside_run = 0;
        } else {
            self.outside_run = self.outside_run.saturating_add(1);
        }
        self.outside_run < 3
    }
}

/// `(cx, cy, inside)` relative to `bounds = (x, y, w, h)`; NOT clamped.
pub fn normalize_point(px: f64, py: f64, bounds: (f64, f64, f64, f64)) -> (f64, f64, bool) {
    let (x, y, w, h) = bounds;
    if !(w > 0.0 && h > 0.0) {
        return (0.0, 0.0, false);
    }
    let cx = (px - x) / w;
    let cy = (py - y) / h;
    let inside = (0.0..=1.0).contains(&cx) && (0.0..=1.0).contains(&cy);
    (cx, cy, inside)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CursorType {
    Arrow,
    Text,
    Pointer,
    Crosshair,
    OpenHand,
    ClosedHand,
    ResizeEw,
    ResizeNs,
    ResizeNesw,
    ResizeNwse,
    Move,
    NotAllowed,
    Wait,
    AppStarting,
    Help,
    UpArrow,
}

fn is_text_input_role(role: Option<&str>) -> bool {
    matches!(
        role,
        Some("AXTextField" | "AXTextArea" | "AXTextView" | "AXComboBox")
    )
}

fn is_pointer_role(role: Option<&str>, subrole: Option<&str>, desc: Option<&str>) -> bool {
    if role == Some("AXLink")
        || subrole.is_some_and(|s| s.to_lowercase().contains("link"))
        || desc.is_some_and(|d| d.contains("link"))
    {
        return true;
    }
    matches!(
        role,
        Some(
            "AXButton"
                | "AXMenuButton"
                | "AXPopUpButton"
                | "AXCheckBox"
                | "AXRadioButton"
                | "AXSwitch"
                | "AXDisclosureTriangle"
                | "AXTab"
                | "AXMenuItem"
        )
    )
}

/// `(role, subrole, roleDescription lowercased)` from the element upwards; max 5 levels.
pub fn cursor_type_for_roles(
    roles: &[(Option<String>, Option<String>, Option<String>)],
) -> Option<CursorType> {
    for (role, subrole, desc) in roles.iter().take(5) {
        if is_text_input_role(role.as_deref()) {
            return Some(CursorType::Text);
        }
        if is_pointer_role(role.as_deref(), subrole.as_deref(), desc.as_deref()) {
            return Some(CursorType::Pointer);
        }
    }
    None
}

// ---------- project dir ----------

fn sanitize_title(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim();
    s.chars().take(80).collect::<String>().trim().to_string()
}

/// `YYYY-MM-DD HH.MM.SS Title`; collision -> ` 2`, ` 3` ...
pub fn project_dir_name(
    now: chrono::DateTime<chrono::Local>,
    title: &str,
    taken: &dyn Fn(&str) -> bool,
) -> String {
    let mut t = sanitize_title(title);
    if t.is_empty() {
        t = crate::i18n::t("rec2.default_title");
    }
    let base = format!("{} {}", now.format("%Y-%m-%d %H.%M.%S"), t);
    if !taken(&base) {
        return base;
    }
    let mut n = 2u32;
    loop {
        let cand = format!("{base} {n}");
        if !taken(&cand) {
            return cand;
        }
        n += 1;
    }
}

// ---------- disk ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskVerdict {
    Ok,
    Low { minutes: u64 },
    Refuse,
}

const GIB: u64 = 1024 * 1024 * 1024;

/// Refuse below 1 GiB free; Low when less than 15 minutes of reserve.
pub fn disk_space_verdict(free_bytes: u64, est_bytes_per_min: u64) -> DiskVerdict {
    if free_bytes < GIB {
        return DiskVerdict::Refuse;
    }
    if est_bytes_per_min == 0 {
        return DiskVerdict::Ok;
    }
    let minutes = free_bytes / est_bytes_per_min;
    if minutes < 15 {
        DiskVerdict::Low { minutes }
    } else {
        DiskVerdict::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn r(role: &str) -> (Option<String>, Option<String>, Option<String>) {
        (Some(role.into()), None, None)
    }

    #[test]
    fn output_size_retina_display() {
        assert_eq!(capture_output_size(1512.0, 982.0, 2.0, 3840, 2160), (3024, 1964));
    }
    #[test]
    fn output_size_caps_proportionally() {
        assert_eq!(capture_output_size(5120.0, 1440.0, 1.0, 3840, 2160), (3840, 1080));
    }
    #[test]
    fn output_size_is_even_and_positive() {
        let (w, h) = capture_output_size(1001.0, 3.0, 1.0, 3840, 2160);
        assert!(w % 2 == 0 && h % 2 == 0 && w >= 2 && h >= 2);
    }
    #[test]
    fn output_size_garbage_falls_back_to_ceiling() {
        assert_eq!(capture_output_size(0.0, f64::NAN, 2.0, 3840, 2160), (3840, 2160));
    }
    #[test]
    fn bitrate_clamps() {
        assert_eq!(default_video_bitrate(1920, 1080, 60), 12_441_600);
        assert_eq!(default_video_bitrate(320, 240, 10), 2_000_000);
        assert_eq!(default_video_bitrate(7680, 4320, 60), 60_000_000);
        assert_eq!(key_frame_interval(0), 1);
        assert_eq!(key_frame_interval(60), 60);
    }
    #[test]
    fn gate_rejects_equal_and_earlier_but_not_after() {
        let mut g = VideoTimestampGate::new();
        assert_eq!(g.check(Some(100)), GateVerdict::Admit);
        g.record(100);
        assert_eq!(g.check(Some(100)), GateVerdict::NotAfterPrevious);
        assert_eq!(g.check(Some(99)), GateVerdict::NotAfterPrevious);
        assert_eq!(g.check(Some(101)), GateVerdict::Admit);
        assert_eq!(g.rejected, 2);
    }
    #[test]
    fn gate_check_does_not_move_last() {
        let mut g = VideoTimestampGate::new();
        assert_eq!(g.check(Some(200)), GateVerdict::Admit);
        assert_eq!(g.check(Some(150)), GateVerdict::Admit);
    }
    #[test]
    fn gate_invalid_none() {
        let mut g = VideoTimestampGate::new();
        assert_eq!(g.check(None), GateVerdict::Invalid);
        assert_eq!(g.rejected, 1);
    }
    #[test]
    fn offset_at_decides_on_capture_time() {
        let closed = [(100, 300), (500, 600)];
        assert_eq!(offset_at(&closed, None, 50), Some(0), "before the first pause");
        assert_eq!(offset_at(&closed, None, 100), None, "pause start is inside");
        assert_eq!(offset_at(&closed, None, 299), None, "inside");
        assert_eq!(offset_at(&closed, None, 300), Some(200), "at the resume: outside");
        assert_eq!(offset_at(&closed, None, 550), None, "inside the second");
        assert_eq!(offset_at(&closed, None, 700), Some(300), "after two pauses: sum");
        // Open pause: a pre-pause sample delivered late keeps offset 0 / 200,
        // anything captured since the pause started is dropped.
        assert_eq!(offset_at(&[], Some(1_000), 999), Some(0), "late-delivered pre-pause sample");
        assert_eq!(offset_at(&closed, Some(1_000), 800), Some(300));
        assert_eq!(offset_at(&closed, Some(1_000), 1_000), None);
        assert_eq!(offset_at(&closed, Some(1_000), 5_000), None);
    }
    #[test]
    fn pause_clock_records_intervals_and_keeps_offset_semantics() {
        let mut c = PauseClock::new();
        c.resume(50); // resume without a pause: nothing
        c.pause(100);
        assert_eq!(c.offset_at(99), Some(0));
        assert_eq!(c.offset_at(150), None);
        c.resume(300);
        c.pause(500);
        c.pause(520); // double pause keeps the first start
        c.resume(600);
        assert_eq!(c.offset_ns(), 300);
        assert_eq!(c.timeline_ns(1_000), 700);
        assert_eq!(c.offset_at(200), None);
        assert_eq!(c.offset_at(400), Some(200));
        assert_eq!(c.offset_at(510), None);
        assert_eq!(c.offset_at(1_000), Some(300));
    }
    #[test]
    fn pause_clock_freezes_and_shifts() {
        let mut c = PauseClock::new();
        assert_eq!(c.timeline_ns(1_000_000_000), 1_000_000_000);
        c.pause(1_000_000_000);
        assert!(c.is_paused());
        assert_eq!(c.timeline_ns(2_000_000_000), c.timeline_ns(3_000_000_000));
        c.resume(3_000_000_000);
        assert!(!c.is_paused());
        assert_eq!(c.offset_ns(), 2_000_000_000);
        assert_eq!(c.timeline_ns(4_000_000_000), 2_000_000_000);
        let s = PauseShared::new();
        s.pause(10);
        s.resume(25);
        assert_eq!(s.offset_ns(), 15);
    }
    #[test]
    fn visibility_hides_after_third_outside_sample() {
        let mut v = CursorVisibility::new();
        assert!(v.observe(false));
        assert!(v.observe(false));
        assert!(!v.observe(false));
        assert!(!v.observe(false));
        assert!(v.observe(true));
        assert!(v.observe(false));
    }
    #[test]
    fn normalize_point_unclamped() {
        let (cx, cy, inside) = normalize_point(150.0, 20.0, (100.0, 0.0, 100.0, 50.0));
        assert!((cx - 0.5).abs() < 1e-9 && (cy - 0.4).abs() < 1e-9 && inside);
        let (cx, _, inside) = normalize_point(250.0, 20.0, (100.0, 0.0, 100.0, 50.0));
        assert!(cx > 1.0 && !inside);
    }
    #[test]
    fn cursor_type_text_and_pointer() {
        assert_eq!(cursor_type_for_roles(&[r("AXTextField")]), Some(CursorType::Text));
        assert_eq!(
            cursor_type_for_roles(&[r("AXGroup"), r("AXGroup"), r("AXButton")]),
            Some(CursorType::Pointer)
        );
        assert_eq!(
            cursor_type_for_roles(&[(Some("AXStaticText".into()), Some("AXSomeLinkThing".into()), None)]),
            Some(CursorType::Pointer)
        );
        let six = vec![r("AXGroup"), r("AXGroup"), r("AXGroup"), r("AXGroup"), r("AXGroup"), r("AXButton")];
        assert_eq!(cursor_type_for_roles(&six), None);
    }
    #[test]
    fn cursor_type_serde_kebab() {
        assert_eq!(serde_json::to_string(&CursorType::ResizeNesw).unwrap(), "\"resize-nesw\"");
        assert_eq!(serde_json::to_string(&CursorType::OpenHand).unwrap(), "\"open-hand\"");
    }
    #[test]
    fn project_dir_name_collides() {
        let now = chrono::Local.with_ymd_and_hms(2026, 11, 1, 9, 5, 7).unwrap();
        let plain = project_dir_name(now, "Demo", &|_| false);
        assert_eq!(plain, "2026-11-01 09.05.07 Demo");
        let p2 = plain.clone();
        let n = project_dir_name(now, "Demo", &move |c| c == p2);
        assert!(n.ends_with(" 2"));
        let clean = project_dir_name(now, "a/b: c", &|_| false);
        assert!(clean.ends_with("a-b- c"), "{clean}");
        let blank = project_dir_name(now, "  ", &|_| false);
        assert!(blank.ends_with(" Recording"), "{blank}");
    }
    #[test]
    fn disk_verdict_thresholds() {
        let mib = 1024 * 1024;
        assert_eq!(disk_space_verdict(500 * mib, 1), DiskVerdict::Refuse);
        assert_eq!(disk_space_verdict(3 * GIB, 300 * mib), DiskVerdict::Low { minutes: 10 });
        assert_eq!(disk_space_verdict(50 * GIB, 300 * mib), DiskVerdict::Ok);
    }
}
