//! `figlet` — ASCII-art banner generator for the search-bar command (v0.85.0).
//!
//! Renders text as FIGlet ASCII art. The heavy lifting (font parsing +
//! smushing/kerning) is the `figlet-rs` crate; everything *around* it —
//! width-wrapping, alignment, trailing-trim, and the dev-extras (box border +
//! comment wrapping) — is our own **pure, unit-tested** logic here, because the
//! crate does none of it.
//!
//! ## Extensibility — the `RenderEngine` trait
//!
//! FIGlet is the first (and, for this ticket, only) implementation of the
//! [`RenderEngine`] trait. A future `boxes`/`cowsay`-style ASCII engine plugs
//! in as another `RenderEngine` without touching the parser, the gallery, or
//! the layout pipeline. The command grammar reserves room for an `@engine`
//! selector so a second engine is reachable without a breaking change.
//!
//! The concrete font *bundle* (hundreds of lazily-decompressed `.flf`) lives in
//! [`fonts`]; this module owns the engine, the render pipeline, and the pure
//! layout helpers. Built-in fonts (`standard`/`slant`/`small`/`big`) let the
//! engine + tests work before the bundle exists.

pub mod fonts;

use std::sync::Arc;

use figlet_rs::FIGlet;
use serde::{Deserialize, Serialize};

use crate::db::DbHandle;
use crate::settings;

// ── Public option / result types ──────────────────────────────────────────

/// Horizontal alignment of the finished banner within the target width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// Optional source-comment wrapping of the banner (for pasting a banner header
/// straight into code). Line-prefix styles (`//`, `#`) are always safe;
/// block styles are best-effort (see the render notes + `docs/figlet.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommentStyle {
    #[default]
    None,
    /// `// ` line prefix (C/JS/Rust/…).
    Slashes,
    /// `# ` line prefix (shell/Python/…).
    Hash,
    /// JSDoc-style `/**` … ` * ` … ` */` block.
    Block,
    /// `<!-- … -->` HTML block.
    Html,
}

/// Rendering options; the frontend fills these from the command flags, with
/// defaults from Settings → Figlet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderOpts {
    /// Wrap/alignment field width in columns. `0` = no wrapping (natural width).
    pub width: usize,
    pub align: Align,
    /// Trim trailing whitespace from each line (clean paste into code).
    pub trim: bool,
    pub comment: CommentStyle,
    /// Draw a box-drawing border around the banner.
    pub boxed: bool,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            width: 80,
            align: Align::Left,
            trim: true,
            comment: CommentStyle::None,
            boxed: false,
        }
    }
}

/// A rendered banner + diagnostics for the preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Banner {
    /// The finished ASCII art, newline-joined. This is *exactly* what Enter
    /// copies to the clipboard — no HTML, no proportional variant.
    pub text: String,
    /// Distinct input characters the font couldn't render (skipped). Drives the
    /// preview's "N characters not available" hint — never a silent loss.
    pub unsupported: Vec<char>,
    /// True if a line had to be wrapped to fit `width`.
    pub wrapped: bool,
}

/// Font metadata exposed to the frontend (never the font bytes themselves).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontMeta {
    pub name: String,
    pub category: String,
    pub popular: bool,
    pub pinned: bool,
}

// ── The engine trait ───────────────────────────────────────────────────────

/// An ASCII-art rendering engine. FIGlet is the first implementation; a second
/// engine (boxes/cowsay-style) can be added without touching the parser or UI.
pub trait RenderEngine: Send + Sync {
    /// Stable engine id (`"figlet"`), reserved for a future `@engine` selector
    /// and asserted by the extensibility test — no production caller yet.
    #[allow(dead_code)]
    fn id(&self) -> &'static str;
    /// The engine's fonts/styles as metadata (no bytes).
    fn fonts(&self) -> Vec<FontMeta>;
    /// Render `text` in `font` with `opts`. Never panics — an unknown font
    /// falls back to the engine default; unrenderable chars are reported.
    fn render(&self, text: &str, font: &str, opts: &RenderOpts) -> Banner;
}

// ── FIGlet engine ──────────────────────────────────────────────────────────

/// The FIGlet implementation of [`RenderEngine`]. Fonts come from [`fonts`]
/// (the lazily-decompressed bundle); the four `figlet-rs` built-ins are always
/// available as a fallback so the engine works with no bundle present.
#[derive(Default)]
pub struct FigletEngine;

impl FigletEngine {
    pub fn new() -> Self {
        Self
    }

    /// Resolve a font by name to a parsed `FIGlet` (Arc-shared, cached). Falls
    /// back to the standard built-in for an unknown name so render never fails.
    fn font(&self, name: &str) -> Arc<FIGlet> {
        fonts::load(name).unwrap_or_else(fonts::standard)
    }
}

impl RenderEngine for FigletEngine {
    fn id(&self) -> &'static str {
        "figlet"
    }

    fn fonts(&self) -> Vec<FontMeta> {
        fonts::catalog()
    }

    fn render(&self, text: &str, font: &str, opts: &RenderOpts) -> Banner {
        let figfont = self.font(font);
        render_with_font(&figfont, text, opts)
    }
}

/// Core render pipeline over an already-resolved font — the seam the tests use
/// directly (no bundle needed): supported-char filter → per-input-line
/// width-wrap → align → box/trim → comment.
pub fn render_with_font(figfont: &FIGlet, text: &str, opts: &RenderOpts) -> Banner {
    let unsupported = unsupported_chars(figfont, text);

    // Render each *input* line as its own banner block, stacked with a blank
    // line between them. Unsupported chars are dropped (figlet skips them
    // anyway) so the surviving art stays aligned — the drop is reported above.
    let mut lines: Vec<String> = Vec::new();
    let mut wrapped = false;
    for (i, input_line) in text.split('\n').enumerate() {
        if i > 0 {
            lines.push(String::new());
        }
        let clean: String = input_line
            .chars()
            .filter(|&c| c == ' ' || is_supported(figfont, c))
            .collect();
        let (block, w) = render_line_wrapped(figfont, &clean, opts.width);
        wrapped |= w;
        lines.extend(block);
    }

    // Alignment field = max(requested width, natural width).
    let natural = lines.iter().map(|l| display_width(l)).max().unwrap_or(0);
    let field = if opts.width > 0 { opts.width.max(natural) } else { natural };
    lines = align_block(lines, field, opts.align);

    if opts.boxed {
        // The box re-pads to a common width and supplies the right border, so
        // there are no raw trailing spaces to trim inside it.
        lines = box_border(lines);
    } else if opts.trim {
        lines = trim_trailing(lines);
    }

    if opts.comment != CommentStyle::None {
        lines = comment_wrap(lines, opts.comment);
    }

    Banner { text: lines.join("\n"), unsupported, wrapped }
}

// ── Settings defaults (bruno pattern: one settings row per field) ───────────

const KEY_FONT: &str = "figlet.font";
const KEY_WIDTH: &str = "figlet.width";
const KEY_ALIGN: &str = "figlet.align";
const KEY_TRIM: &str = "figlet.trim";
const KEY_COMMENT: &str = "figlet.comment";
const KEY_BOXED: &str = "figlet.boxed";
const KEY_PINNED: &str = "figlet.pinned";
const KEY_SAVE_HISTORY: &str = "figlet.save_history";

/// Per-user Figlet defaults, applied to a bare `figlet <text>` invocation and
/// as the starting point for the preview's option chips. Persisted one row per
/// field in the `settings` table; edited in Settings → Figlet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FigletDefaults {
    pub font: String,
    pub width: usize,
    pub align: Align,
    pub trim: bool,
    pub comment: CommentStyle,
    pub boxed: bool,
    /// Font names pinned to the top of the gallery.
    pub pinned: Vec<String>,
    /// Save `figlet` results to clipboard history (consistent with all commands).
    pub save_history: bool,
}

impl Default for FigletDefaults {
    fn default() -> Self {
        Self {
            font: "standard".to_string(),
            width: 80,
            align: Align::Left,
            trim: true,
            comment: CommentStyle::None,
            boxed: false,
            pinned: Vec::new(),
            save_history: true,
        }
    }
}

fn align_from_str(s: &str) -> Align {
    match s {
        "center" => Align::Center,
        "right" => Align::Right,
        _ => Align::Left,
    }
}

fn align_str(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

fn comment_from_str(s: &str) -> CommentStyle {
    match s {
        "slashes" => CommentStyle::Slashes,
        "hash" => CommentStyle::Hash,
        "block" => CommentStyle::Block,
        "html" => CommentStyle::Html,
        _ => CommentStyle::None,
    }
}

fn comment_str(c: CommentStyle) -> &'static str {
    match c {
        CommentStyle::None => "none",
        CommentStyle::Slashes => "slashes",
        CommentStyle::Hash => "hash",
        CommentStyle::Block => "block",
        CommentStyle::Html => "html",
    }
}

pub fn get_defaults(db: &DbHandle) -> anyhow::Result<FigletDefaults> {
    let d = FigletDefaults::default();
    Ok(FigletDefaults {
        font: settings::get_or(db, KEY_FONT, &d.font).unwrap_or(d.font.clone()),
        width: settings::get_or(db, KEY_WIDTH, &d.width.to_string())
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(d.width),
        align: align_from_str(&settings::get_or(db, KEY_ALIGN, align_str(d.align)).unwrap_or_default()),
        trim: settings::get_bool(db, KEY_TRIM, d.trim).unwrap_or(d.trim),
        comment: comment_from_str(
            &settings::get_or(db, KEY_COMMENT, comment_str(d.comment)).unwrap_or_default(),
        ),
        boxed: settings::get_bool(db, KEY_BOXED, d.boxed).unwrap_or(d.boxed),
        pinned: settings::get_or(db, KEY_PINNED, "[]")
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        save_history: settings::get_bool(db, KEY_SAVE_HISTORY, d.save_history).unwrap_or(d.save_history),
    })
}

pub fn set_defaults(db: &DbHandle, defs: &FigletDefaults) -> anyhow::Result<()> {
    // Only persist a known font name (defensive against a stale/garbage value).
    let font = if fonts::load(&defs.font).is_some() {
        defs.font.clone()
    } else {
        "standard".to_string()
    };
    // Keep only pinned names that actually exist, deduped, capped.
    let mut pinned: Vec<String> = Vec::new();
    for name in &defs.pinned {
        if pinned.len() >= 64 {
            break;
        }
        if fonts::load(name).is_some() && !pinned.contains(name) {
            pinned.push(name.clone());
        }
    }
    let width = defs.width.clamp(0, 400);

    settings::set(db, KEY_FONT, &font)?;
    settings::set(db, KEY_WIDTH, &width.to_string())?;
    settings::set(db, KEY_ALIGN, align_str(defs.align))?;
    settings::set(db, KEY_TRIM, if defs.trim { "true" } else { "false" })?;
    settings::set(db, KEY_COMMENT, comment_str(defs.comment))?;
    settings::set(db, KEY_BOXED, if defs.boxed { "true" } else { "false" })?;
    settings::set(db, KEY_PINNED, &serde_json::to_string(&pinned)?)?;
    settings::set(db, KEY_SAVE_HISTORY, if defs.save_history { "true" } else { "false" })?;
    Ok(())
}

// ── Convenience API for the IPC layer ───────────────────────────────────────

/// A compact gallery sample: the font name + a truncated banner of the user's
/// text in that font.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FigletSample {
    pub font: String,
    pub sample: String,
}

/// Render the big preview / copy payload (delegates to [`FigletEngine`]).
pub fn render(text: &str, font: &str, opts: &RenderOpts) -> Banner {
    FigletEngine::new().render(text, font, opts)
}

/// Font metadata with the user's pinned set applied. Enumerated through the
/// engine's [`RenderEngine::fonts`] so the trait's font-enumeration surface is
/// the one production path (a future engine reuses this shape unchanged).
pub fn fonts_meta(pinned: &[String]) -> Vec<FontMeta> {
    let pins: std::collections::HashSet<&str> = pinned.iter().map(String::as_str).collect();
    FigletEngine::new()
        .fonts()
        .into_iter()
        .map(|mut m| {
            m.pinned = pins.contains(m.name.as_str());
            m
        })
        .collect()
}

/// Compact samples for a set of fonts (the gallery rows). Each sample ignores
/// the box/comment chrome (that's for the big preview) and is capped to
/// `max_lines` × `max_cols` so hundreds of rows stay cheap. `…` marks a cut.
pub fn gallery(text: &str, font_names: &[String], max_lines: usize, max_cols: usize) -> Vec<FigletSample> {
    let engine = FigletEngine::new();
    let sample_opts = RenderOpts { width: 0, align: Align::Left, trim: true, comment: CommentStyle::None, boxed: false };
    font_names
        .iter()
        .map(|font| {
            let banner = engine.render(text, font, &sample_opts);
            let sample = truncate_sample(&banner.text, max_lines, max_cols);
            FigletSample { font: font.clone(), sample }
        })
        .collect()
}

/// Truncate a banner to a compact gallery preview (first `max_lines` lines, each
/// clipped to `max_cols` columns).
fn truncate_sample(banner: &str, max_lines: usize, max_cols: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    let total = banner.lines().count();
    for line in banner.lines().take(max_lines) {
        if display_width(line) > max_cols {
            let clipped: String = line.chars().take(max_cols.saturating_sub(1)).collect();
            out.push(format!("{clipped}…"));
        } else {
            out.push(line.to_string());
        }
    }
    if total > max_lines {
        out.push("…".to_string());
    }
    out.join("\n")
}

// ── Pure helpers (engine-agnostic, exhaustively unit-tested) ────────────────

/// Whether the font can render a single character (whitespace is always ok).
fn is_supported(font: &FIGlet, c: char) -> bool {
    if c.is_whitespace() {
        return true;
    }
    match font.convert(&c.to_string()) {
        Some(fig) => !fig.to_string().trim().is_empty(),
        None => false,
    }
}

/// Distinct, order-preserving list of characters the font can't render.
pub fn unsupported_chars(font: &FIGlet, text: &str) -> Vec<char> {
    let mut seen: Vec<char> = Vec::new();
    for c in text.chars() {
        if c == '\n' || c == ' ' || is_supported(font, c) {
            continue;
        }
        if !seen.contains(&c) {
            seen.push(c);
        }
    }
    seen
}

/// Render one already-cleaned input line, wrapping at `width` on word
/// boundaries. Each wrapped row is a *full* FIGlet render of its word group
/// (so the font's smushing stays intact); rows are stacked vertically. Returns
/// the block lines + whether any wrap happened. `width == 0` disables wrapping.
fn render_line_wrapped(font: &FIGlet, line: &str, width: usize) -> (Vec<String>, bool) {
    let render = |s: &str| -> Vec<String> {
        font.convert(s)
            .map(|f| f.to_string().split('\n').map(str::to_string).collect())
            .unwrap_or_default()
    };
    let measure = |s: &str| render(s).iter().map(|l| display_width(l)).max().unwrap_or(0);

    let groups = wrap_words(line, width, &measure);
    let mut out: Vec<String> = Vec::new();
    let wrapped = groups.len() > 1;
    for (i, g) in groups.iter().enumerate() {
        if i > 0 {
            out.push(String::new());
        }
        let block = render(g);
        // A blank group (all-unsupported / empty) renders nothing — keep a
        // single blank line so structure survives.
        if block.iter().all(|l| l.trim().is_empty()) && g.trim().is_empty() {
            out.push(String::new());
        } else {
            out.extend(block);
        }
    }
    (out, wrapped)
}

/// Greedily group whitespace-separated words so that each group's rendered
/// width (per `measure`) is ≤ `width`. A single word wider than `width` gets its
/// own group (we never break mid-word — better an overflow than shredded art).
/// `width == 0` returns the whole line as one group.
pub fn wrap_words(line: &str, width: usize, measure: &dyn Fn(&str) -> usize) -> Vec<String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    if width == 0 || words.is_empty() {
        let joined = words.join(" ");
        return vec![if joined.is_empty() { line.to_string() } else { joined }];
    }
    let mut groups: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in words {
        let candidate = if cur.is_empty() { w.to_string() } else { format!("{cur} {w}") };
        if cur.is_empty() || measure(&candidate) <= width {
            cur = candidate;
        } else {
            groups.push(std::mem::take(&mut cur));
            cur = w.to_string();
        }
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    if groups.is_empty() {
        groups.push(line.to_string());
    }
    groups
}

/// Pad every line to `field` columns for the given alignment. Left keeps the
/// natural left edge; center/right add left padding.
pub fn align_block(lines: Vec<String>, field: usize, align: Align) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            let w = display_width(&line);
            if w >= field {
                return line;
            }
            let pad = field - w;
            match align {
                Align::Left => format!("{line}{}", " ".repeat(pad)),
                Align::Right => format!("{}{line}", " ".repeat(pad)),
                Align::Center => {
                    let left = pad / 2;
                    let right = pad - left;
                    format!("{}{line}{}", " ".repeat(left), " ".repeat(right))
                }
            }
        })
        .collect()
}

/// Right-trim each line (remove trailing spaces only); internal structure and
/// left padding are untouched.
pub fn trim_trailing(lines: Vec<String>) -> Vec<String> {
    lines.into_iter().map(|l| l.trim_end().to_string()).collect()
}

/// Wrap the banner in a box-drawing border, flush to the widest line.
pub fn box_border(lines: Vec<String>) -> Vec<String> {
    let inner = lines.iter().map(|l| display_width(l)).max().unwrap_or(0);
    let horiz = "─".repeat(inner + 2);
    let mut out = Vec::with_capacity(lines.len() + 2);
    out.push(format!("┌{horiz}┐"));
    for l in &lines {
        let pad = inner - display_width(l);
        out.push(format!("│ {l}{} │", " ".repeat(pad)));
    }
    out.push(format!("└{horiz}┘"));
    out
}

/// Wrap the banner as a source comment. Line-prefix styles are always valid;
/// block styles guard their closing delimiter so the block stays re-parseable.
pub fn comment_wrap(lines: Vec<String>, style: CommentStyle) -> Vec<String> {
    match style {
        CommentStyle::None => lines,
        CommentStyle::Slashes => lines.into_iter().map(|l| format!("// {l}").trim_end().to_string()).collect(),
        CommentStyle::Hash => lines.into_iter().map(|l| format!("# {l}").trim_end().to_string()).collect(),
        CommentStyle::Block => {
            let mut out = Vec::with_capacity(lines.len() + 2);
            out.push("/**".to_string());
            for l in &lines {
                // Guard an accidental `*/` inside the art from closing early.
                let safe = l.replace("*/", "* /");
                out.push(format!(" * {safe}").trim_end().to_string());
            }
            out.push(" */".to_string());
            out
        }
        CommentStyle::Html => {
            let mut out = Vec::with_capacity(lines.len() + 2);
            out.push("<!--".to_string());
            for l in &lines {
                // Guard `-->` inside the art from closing the comment early.
                out.push(l.replace("-->", "-- >").trim_end().to_string());
            }
            out.push("-->".to_string());
            out
        }
    }
}

/// Display width of a line in monospace columns. FIGlet output plus our
/// box-drawing chars are all single-width, so this is `chars().count()` (byte
/// length would over-count the multi-byte box glyphs).
fn display_width(s: &str) -> usize {
    s.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn std_font() -> Arc<FIGlet> {
        fonts::standard()
    }

    fn render(text: &str, opts: &RenderOpts) -> Banner {
        render_with_font(&std_font(), text, opts)
    }

    #[test]
    fn renders_non_empty_multiline_banner() {
        let b = render("Hi", &RenderOpts::default());
        assert!(!b.text.is_empty());
        assert!(b.text.lines().count() >= 3, "banner should be multi-line");
        assert!(b.unsupported.is_empty());
    }

    #[test]
    fn deterministic() {
        let o = RenderOpts::default();
        assert_eq!(render("Hello", &o).text, render("Hello", &o).text);
    }

    #[test]
    fn engine_satisfies_trait_and_reports_id() {
        let e = FigletEngine::new();
        assert_eq!(e.id(), "figlet");
        let b = e.render("Hi", "standard", &RenderOpts::default());
        assert!(!b.text.is_empty());
    }

    #[test]
    fn unknown_font_falls_back_not_panics() {
        let e = FigletEngine::new();
        let b = e.render("Hi", "this-font-does-not-exist", &RenderOpts::default());
        assert!(!b.text.is_empty());
    }

    // ── unsupported chars ────────────────────────────────────────────────

    #[test]
    fn unsupported_char_is_reported_not_panicked() {
        // Emoji / CJK aren't in standard.flf.
        let b = render("Hi🎉", &RenderOpts::default());
        assert!(b.unsupported.contains(&'🎉'));
        assert!(!b.text.is_empty(), "the supported part still renders");
    }

    #[test]
    fn all_unsupported_yields_empty_but_flagged() {
        let b = render("世界", &RenderOpts::default());
        assert_eq!(b.unsupported, vec!['世', '界']);
        assert!(b.text.trim().is_empty());
    }

    #[test]
    fn unsupported_is_deduped_and_ordered() {
        let b = render("a世a界世", &RenderOpts::default());
        assert_eq!(b.unsupported, vec!['世', '界']);
    }

    // ── alignment ────────────────────────────────────────────────────────

    #[test]
    fn alignment_changes_output() {
        let base = RenderOpts { width: 60, trim: false, ..Default::default() };
        let left = render("Hi", &RenderOpts { align: Align::Left, ..base.clone() }).text;
        let center = render("Hi", &RenderOpts { align: Align::Center, ..base.clone() }).text;
        let right = render("Hi", &RenderOpts { align: Align::Right, ..base.clone() }).text;
        assert_ne!(left, center);
        assert_ne!(center, right);
        assert_ne!(left, right);
        // Right-aligned lines end at the field width; left-aligned start at col 0.
        let right_line = right.lines().find(|l| !l.trim().is_empty()).unwrap();
        assert!(right_line.starts_with(' '), "right align pads on the left");
    }

    #[test]
    fn align_block_pads_correctly() {
        let lines = vec!["ab".to_string(), "c".to_string()];
        assert_eq!(align_block(lines.clone(), 4, Align::Left), vec!["ab  ", "c   "]);
        assert_eq!(align_block(lines.clone(), 4, Align::Right), vec!["  ab", "   c"]);
        assert_eq!(align_block(lines, 5, Align::Center), vec![" ab  ", "  c  "]);
    }

    // ── trailing trim ────────────────────────────────────────────────────

    #[test]
    fn trim_removes_only_right_spaces() {
        let lines = vec!["  ab  ".to_string(), "cd\t ".to_string()];
        assert_eq!(trim_trailing(lines), vec!["  ab", "cd"]);
    }

    #[test]
    fn trim_default_on_removes_trailing() {
        let raw = render("Hi", &RenderOpts { trim: false, width: 0, ..Default::default() }).text;
        let trimmed = render("Hi", &RenderOpts { trim: true, width: 0, ..Default::default() }).text;
        assert!(raw.lines().any(|l| l.ends_with(' ')), "raw figlet has trailing spaces");
        assert!(trimmed.lines().all(|l| !l.ends_with(' ')), "trim removed them");
    }

    // ── width wrapping ───────────────────────────────────────────────────

    #[test]
    fn width_wraps_not_truncates() {
        // A narrow width forces the two words onto separate rows; nothing is lost.
        let narrow = render("Hello World", &RenderOpts { width: 30, trim: true, ..Default::default() });
        assert!(narrow.wrapped, "should have wrapped");
        // Every rendered line stays within the width.
        for l in narrow.text.lines() {
            assert!(display_width(l) <= 30, "line {:?} exceeds width", l);
        }
        // Wide enough → single group, no wrap.
        let wide = render("Hello World", &RenderOpts { width: 200, ..Default::default() });
        assert!(!wide.wrapped);
    }

    #[test]
    fn wrap_words_groups_greedily() {
        // measure = 1 col per char of the joined string.
        let m = |s: &str| s.chars().count();
        assert_eq!(wrap_words("aa bb cc", 5, &m), vec!["aa bb", "cc"]);
        assert_eq!(wrap_words("aa bb cc", 100, &m), vec!["aa bb cc"]);
        // width 0 = one group.
        assert_eq!(wrap_words("aa bb", 0, &m), vec!["aa bb"]);
        // A single over-width word gets its own group, unbroken.
        assert_eq!(wrap_words("aaaaaa bb", 3, &m), vec!["aaaaaa", "bb"]);
    }

    // ── box border ───────────────────────────────────────────────────────

    #[test]
    fn box_encloses_flush_with_uneven_lines() {
        let lines = vec!["ab".to_string(), "cdef".to_string(), "g".to_string()];
        let boxed = box_border(lines);
        // Top + bottom + 3 content = 5 lines, all equal display width.
        assert_eq!(boxed.len(), 5);
        let w = display_width(&boxed[0]);
        assert!(boxed.iter().all(|l| display_width(l) == w), "box is flush");
        assert!(boxed[0].starts_with('┌') && boxed[0].ends_with('┐'));
        assert!(boxed[4].starts_with('└') && boxed[4].ends_with('┘'));
        assert!(boxed[1].starts_with('│') && boxed[1].ends_with('│'));
    }

    // ── comment wrapping ─────────────────────────────────────────────────

    #[test]
    fn comment_slashes_and_hash_prefix_each_line() {
        let lines = vec!["ab".to_string(), "cd".to_string()];
        assert_eq!(
            comment_wrap(lines.clone(), CommentStyle::Slashes),
            vec!["// ab", "// cd"]
        );
        assert_eq!(comment_wrap(lines, CommentStyle::Hash), vec!["# ab", "# cd"]);
    }

    #[test]
    fn comment_block_is_valid_and_guards_delimiter() {
        let lines = vec!["a*/b".to_string()];
        let out = comment_wrap(lines, CommentStyle::Block);
        assert_eq!(out.first().unwrap(), "/**");
        assert_eq!(out.last().unwrap(), " */");
        // No premature `*/` in the body.
        assert!(!out[1].contains("*/"), "the inner `*/` was neutralised");
    }

    #[test]
    fn comment_html_guards_close() {
        let out = comment_wrap(vec!["a-->b".to_string()], CommentStyle::Html);
        assert_eq!(out.first().unwrap(), "<!--");
        assert_eq!(out.last().unwrap(), "-->");
        assert!(!out[1].contains("-->"));
    }

    #[test]
    fn box_then_comment_combined_is_recommentable() {
        // Box first, then comment-wrap the whole boxed block.
        let banner = render("Hi", &RenderOpts {
            boxed: true,
            comment: CommentStyle::Slashes,
            width: 0,
            ..Default::default()
        });
        let lines: Vec<&str> = banner.text.lines().collect();
        assert!(lines.iter().all(|l| l.starts_with("//")), "every line commented");
        // Un-comment and confirm the box border survived underneath.
        let uncommented: Vec<String> =
            lines.iter().map(|l| l.trim_start_matches("//").trim_start().to_string()).collect();
        assert!(uncommented.iter().any(|l| l.starts_with('┌')));
        assert!(uncommented.iter().any(|l| l.starts_with('└')));
    }

    // ── RenderEngine extensibility (proves a 2nd engine needs no UI change) ─

    struct DummyEngine;
    impl RenderEngine for DummyEngine {
        fn id(&self) -> &'static str {
            "dummy"
        }
        fn fonts(&self) -> Vec<FontMeta> {
            vec![FontMeta { name: "block".into(), category: "test".into(), popular: true, pinned: false }]
        }
        fn render(&self, text: &str, _font: &str, _opts: &RenderOpts) -> Banner {
            Banner { text: format!("[{text}]"), unsupported: vec![], wrapped: false }
        }
    }

    // ── settings defaults + gallery ──────────────────────────────────────

    fn mem_db() -> DbHandle {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let h = std::sync::Arc::new(parking_lot::Mutex::new(conn));
        settings::init_table(&h).unwrap();
        h
    }

    #[test]
    fn defaults_round_trip() {
        let db = mem_db();
        let custom = FigletDefaults {
            font: "slant".into(),
            width: 120,
            align: Align::Center,
            trim: false,
            comment: CommentStyle::Slashes,
            boxed: true,
            pinned: vec!["slant".into(), "big".into()],
            save_history: false,
        };
        set_defaults(&db, &custom).unwrap();
        let back = get_defaults(&db).unwrap();
        assert_eq!(back.font, "slant");
        assert_eq!(back.width, 120);
        assert_eq!(back.align, Align::Center);
        assert!(!back.trim);
        assert_eq!(back.comment, CommentStyle::Slashes);
        assert!(back.boxed);
        assert_eq!(back.pinned, vec!["slant".to_string(), "big".to_string()]);
        assert!(!back.save_history);
    }

    #[test]
    fn defaults_fallback_when_unset() {
        let d = get_defaults(&mem_db()).unwrap();
        assert_eq!(d.font, "standard");
        assert_eq!(d.align, Align::Left);
        assert!(d.trim);
        assert!(d.save_history);
    }

    #[test]
    fn set_defaults_drops_unknown_font_and_pins() {
        let db = mem_db();
        set_defaults(
            &db,
            &FigletDefaults {
                font: "not-a-real-font".into(),
                pinned: vec!["standard".into(), "also-fake".into(), "standard".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let back = get_defaults(&db).unwrap();
        assert_eq!(back.font, "standard", "unknown font coerced to standard");
        assert_eq!(back.pinned, vec!["standard".to_string()], "fake/dupe pins dropped");
    }

    #[test]
    fn fonts_meta_applies_pins() {
        let meta = fonts_meta(&["slant".to_string()]);
        assert!(meta.iter().find(|m| m.name == "slant").unwrap().pinned);
        assert!(!meta.iter().find(|m| m.name == "standard").unwrap().pinned);
    }

    #[test]
    fn gallery_returns_compact_samples() {
        let samples = gallery("Hi", &["standard".into(), "slant".into()], 3, 40);
        assert_eq!(samples.len(), 2);
        for s in &samples {
            assert!(!s.sample.is_empty());
            // Capped to ≤ max_lines + a possible ellipsis marker line.
            assert!(s.sample.lines().count() <= 4);
            assert!(s.sample.lines().all(|l| l.chars().count() <= 40));
        }
    }

    #[test]
    fn truncate_sample_clips_lines_and_cols() {
        let banner = "aaaaaaaaaa\nbbbbbbbbbb\ncccccccccc\ndddddddddd";
        let s = truncate_sample(banner, 2, 5);
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines[0], "aaaa…"); // clipped to 5 cols
        assert_eq!(lines[1], "bbbb…");
        assert_eq!(lines[2], "…"); // more-lines marker
    }

    #[test]
    fn a_second_engine_plugs_into_the_trait() {
        // A different RenderEngine implementation works through the same trait
        // object the command dispatch uses — no parser/UI change required.
        let engines: Vec<Box<dyn RenderEngine>> = vec![Box::new(FigletEngine::new()), Box::new(DummyEngine)];
        assert_eq!(engines[0].id(), "figlet");
        assert_eq!(engines[1].id(), "dummy");
        assert_eq!(engines[1].render("x", "block", &RenderOpts::default()).text, "[x]");
        assert_eq!(engines[1].fonts()[0].name, "block");
    }
}
