//! Passive auto-expansion — the aText / Espanso / TextExpander "it just
//! works" mode (v0.56.0+).
//!
//! Unlike the three existing expander modes (in-popup search, the
//! abbreviation **hotkey**, and direct-slot hotkeys), this mode requires
//! **no deliberate action**: a system-wide keystroke monitor watches what
//! the user types into *any* app, keeps the last few characters in a small
//! ring buffer, matches them against the snippet abbreviations, and expands
//! automatically the moment the abbreviation is completed (immediate) or
//! followed by a delimiter (the default).
//!
//! ## Architecture
//!
//! - **Pure core** (this module's top half) — `AbbrevTable`, `TypeBuffer`,
//!   `AutoExpander`, delimiter classification. Time-free, FFI-free, fully
//!   unit-tested. `AutoExpander::feed` is a pure state machine returning an
//!   [`ExpandAction`]; the impure executor (`perform_expansion`) acts on it.
//! - **Platform key monitor** ([`KeyMonitor`]) — macOS `CGEventTap` (the same
//!   raw-FFI pattern proven in `input_lock.rs`), Windows `SetWindowsHookEx`
//!   (`WH_KEYBOARD_LL`), and a no-op stub elsewhere. Each feeds decoded
//!   characters / control events into the engine.
//! - **State + lifecycle** ([`AutoExpandState`]) — managed by Tauri; started
//!   from `lib.rs` when the setting is on and (on macOS) Accessibility is
//!   granted; the abbreviation table is rebuilt whenever snippets change.
//!
//! ## Safety
//!
//! - **Never** expands in a secure/password field (`text_field::is_focused_field_secure`).
//! - The buffer is cleared on focus change, mouse click and navigation keys
//!   so a stale prefix can't trigger a wrong match.
//! - Synthesized keystrokes (our own backspaces + paste) are ignored by the
//!   monitor (Windows: the `LLKHF_INJECTED` flag; macOS: an `INJECTING`
//!   guard with a short grace window) so an expansion can't recursively
//!   trigger itself.

use serde::{Deserialize, Serialize};

use crate::db::DbHandle;

// ─────────────────────────────────────────────────────────────────────────
//  Settings
// ─────────────────────────────────────────────────────────────────────────

/// Master on/off for passive auto-expansion.
pub const KEY_ENABLED: &str = "expander.auto_expand_enabled";
/// `"delimiter"` (default) or `"immediate"`.
pub const KEY_TRIGGER: &str = "expander.auto_expand_trigger";
/// Match abbreviations case-sensitively (default `false` → case-insensitive).
pub const KEY_MATCH_CASE: &str = "expander.auto_expand_match_case";
/// Allow an abbreviation to fire even when it's glued to the end of a longer
/// word (default `false` → only standalone words expand).
pub const KEY_INSIDE_WORDS: &str = "expander.auto_expand_inside_words";
/// A single Backspace immediately after an expansion restores the
/// abbreviation (aText behaviour; default `true`).
pub const KEY_UNDO: &str = "expander.auto_expand_undo";

/// Largest number of recent characters kept in the ring buffer. The longest
/// abbreviation we can ever match is bounded by this. 64 comfortably covers
/// every realistic abbreviation while keeping the per-keystroke scan trivial.
pub const BUFFER_CAP: usize = 64;

/// When does a fully-typed abbreviation expand?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    /// Expand only once the abbreviation is followed by a delimiter
    /// (space / tab / enter / punctuation). The default — prevents an
    /// abbreviation that is a prefix of a real word from firing early.
    Delimiter,
    /// Expand the instant the abbreviation is fully typed, no delimiter
    /// needed.
    Immediate,
}

impl Trigger {
    pub fn from_str_loose(s: &str) -> Trigger {
        match s.trim().to_ascii_lowercase().as_str() {
            "immediate" => Trigger::Immediate,
            _ => Trigger::Delimiter,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Delimiter => "delimiter",
            Trigger::Immediate => "immediate",
        }
    }
}

/// Auto-expansion configuration, mirrored from the settings store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoExpandConfig {
    pub enabled: bool,
    pub trigger: Trigger,
    pub match_case: bool,
    pub expand_inside_words: bool,
    pub undo_enabled: bool,
}

impl Default for AutoExpandConfig {
    fn default() -> Self {
        AutoExpandConfig {
            enabled: false,
            trigger: Trigger::Delimiter,
            match_case: false,
            expand_inside_words: false,
            undo_enabled: true,
        }
    }
}

/// Read the config from settings, falling back to defaults for any key that
/// is missing or unparsable.
pub fn load_config(db: &DbHandle) -> AutoExpandConfig {
    let d = AutoExpandConfig::default();
    AutoExpandConfig {
        enabled: crate::settings::get_bool(db, KEY_ENABLED, d.enabled).unwrap_or(d.enabled),
        trigger: crate::settings::get(db, KEY_TRIGGER)
            .ok()
            .flatten()
            .map(|s| Trigger::from_str_loose(&s))
            .unwrap_or(d.trigger),
        match_case: crate::settings::get_bool(db, KEY_MATCH_CASE, d.match_case)
            .unwrap_or(d.match_case),
        expand_inside_words: crate::settings::get_bool(db, KEY_INSIDE_WORDS, d.expand_inside_words)
            .unwrap_or(d.expand_inside_words),
        undo_enabled: crate::settings::get_bool(db, KEY_UNDO, d.undo_enabled).unwrap_or(d.undo_enabled),
    }
}

/// Persist the config to settings (one row per field, like the other
/// dotted-key settings).
pub fn save_config(db: &DbHandle, cfg: &AutoExpandConfig) -> anyhow::Result<()> {
    crate::settings::set(db, KEY_ENABLED, if cfg.enabled { "true" } else { "false" })?;
    crate::settings::set(db, KEY_TRIGGER, cfg.trigger.as_str())?;
    crate::settings::set(db, KEY_MATCH_CASE, if cfg.match_case { "true" } else { "false" })?;
    crate::settings::set(
        db,
        KEY_INSIDE_WORDS,
        if cfg.expand_inside_words { "true" } else { "false" },
    )?;
    crate::settings::set(db, KEY_UNDO, if cfg.undo_enabled { "true" } else { "false" })?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
//  Pure core — abbreviation table
// ─────────────────────────────────────────────────────────────────────────

/// One abbreviation → snippet binding, pre-decomposed into `char`s so the
/// per-keystroke suffix scan is a cheap slice compare (multibyte-safe — an
/// umlaut or emoji counts as one element, matching how we count Backspaces).
#[derive(Debug, Clone)]
struct Entry {
    chars: Vec<char>,
    snippet_id: i64,
}

/// In-memory abbreviation index, rebuilt from the `snippets` table whenever
/// snippets change. Entries are sorted **longest-first** so a linear scan
/// naturally honours "longest abbreviation wins" (e.g. `addr` beats `ad`).
#[derive(Debug, Clone, Default)]
pub struct AbbrevTable {
    entries: Vec<Entry>,
}

impl AbbrevTable {
    /// Build from `(abbreviation, snippet_id)` pairs. Empty abbreviations are
    /// skipped. The result is sorted by descending char length.
    pub fn from_pairs<I, S>(pairs: I) -> AbbrevTable
    where
        I: IntoIterator<Item = (S, i64)>,
        S: AsRef<str>,
    {
        let mut entries: Vec<Entry> = pairs
            .into_iter()
            .filter_map(|(abbr, id)| {
                let chars: Vec<char> = abbr.as_ref().chars().collect();
                if chars.is_empty() {
                    None
                } else {
                    Some(Entry { chars, snippet_id: id })
                }
            })
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.chars.len()));
        AbbrevTable { entries }
    }

    /// Build the table from the live snippets table.
    pub fn from_db(db: &DbHandle) -> AbbrevTable {
        match crate::snippets::list_all(db) {
            Ok(list) => AbbrevTable::from_pairs(list.into_iter().map(|s| (s.abbreviation, s.id))),
            Err(e) => {
                tracing::warn!("auto_expand: building abbreviation table failed: {e:#}");
                AbbrevTable::default()
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Find the longest abbreviation that is a suffix of `buf`.
    ///
    /// - `match_case = false` compares ASCII-case-insensitively.
    /// - `inside_words = false` requires the char *before* the matched
    ///   suffix to be a word boundary (start of buffer or a non-word char),
    ///   so `mfg` won't fire as the tail of `chmfg`.
    ///
    /// Returns `(snippet_id, abbreviation_char_len)`.
    pub fn match_suffix(
        &self,
        buf: &[char],
        match_case: bool,
        inside_words: bool,
    ) -> Option<(i64, usize)> {
        if buf.is_empty() {
            return None;
        }
        for e in &self.entries {
            let n = e.chars.len();
            if n > buf.len() {
                continue;
            }
            let tail = &buf[buf.len() - n..];
            if !chars_eq(tail, &e.chars, match_case) {
                continue;
            }
            if !inside_words {
                // The char immediately before the matched suffix must be a
                // boundary (or the suffix must start the buffer).
                if buf.len() > n {
                    let prev = buf[buf.len() - n - 1];
                    if is_word_char(prev) {
                        continue;
                    }
                }
            }
            return Some((e.snippet_id, n));
        }
        None
    }
}

/// Equal-length char-slice comparison, optionally ASCII-case-insensitive.
fn chars_eq(a: &[char], b: &[char], match_case: bool) -> bool {
    if a.len() != b.len() {
        return false;
    }
    if match_case {
        a == b
    } else {
        a.iter()
            .zip(b.iter())
            .all(|(x, y)| x.eq_ignore_ascii_case(y))
    }
}

/// A "word" char for boundary detection: alphanumeric (any script) or `_`.
/// Everything else (space, punctuation, symbols) is a boundary.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Is `c` a delimiter that triggers a delimiter-mode expansion? Whitespace
/// and common ASCII punctuation. (A delimiter is also, by definition, not a
/// word char — but not every non-word char is a *typed* delimiter; control
/// chars are handled separately as `KeyEvent`s.)
pub fn is_delimiter(c: char) -> bool {
    c == ' '
        || c == '\t'
        || c == '\n'
        || c == '\r'
        || matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | '(' | ')' | '[' | ']' | '{' | '}'
                | '"' | '\'' | '/' | '\\' | '-' | '=' | '+' | '<' | '>' | '@'
                | '#' | '$' | '%' | '&' | '*' | '|' | '~' | '`'
        )
}

// ─────────────────────────────────────────────────────────────────────────
//  Pure core — the engine
// ─────────────────────────────────────────────────────────────────────────

/// A decoded event from the platform key monitor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyEvent {
    /// A printable character was typed (already layout-decoded).
    Char(char),
    /// Backspace / Delete-back.
    Backspace,
    /// A key that should clear the buffer without contributing a char:
    /// arrows, Home/End, Page, Enter-as-navigation in some apps, Esc, Tab
    /// used for focus, etc. Also fired on focus change and mouse click by
    /// the monitor.
    Reset,
}

/// What the engine wants the executor to do in response to a fed event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandAction {
    /// Nothing to do.
    None,
    /// Replace a just-typed abbreviation with the snippet body.
    Expand {
        snippet_id: i64,
        /// How many characters to delete before pasting: the abbreviation,
        /// plus the trailing delimiter when one triggered the expansion (we
        /// re-emit the delimiter after the body so it isn't swallowed).
        backspaces: usize,
        /// The delimiter to re-type after the body (delimiter mode), or
        /// `None` (immediate mode).
        trailing: Option<char>,
    },
    /// Undo the previous expansion (a Backspace was pressed right after one):
    /// remove the inserted text and restore what the user originally typed.
    /// The monitor **consumes** the triggering Backspace so it doesn't also
    /// delete a character.
    Undo {
        /// Characters of inserted text to remove (rendered body + any
        /// re-emitted trailing delimiter).
        delete_chars: usize,
        /// The text to type back in its place (abbreviation + the trailing
        /// delimiter that triggered the expansion, if any).
        restore_text: String,
    },
}

/// Record of the last expansion, kept only long enough to support a single
/// "undo on the next Backspace" (aText behaviour).
#[derive(Debug, Clone)]
struct LastExpansion {
    /// Abbreviation + trailing delimiter — what gets typed back on undo.
    restore_text: String,
    /// Total inserted chars (rendered body + trailing) — what undo deletes.
    inserted_chars: usize,
    /// Set false once the very next event passes, so undo only applies
    /// *immediately* after the expansion.
    fresh: bool,
}

/// The pure auto-expansion state machine. Drive it with [`feed`](Self::feed);
/// it owns the type buffer and the (rebuildable) abbreviation table.
#[derive(Debug)]
pub struct AutoExpander {
    table: AbbrevTable,
    cfg: AutoExpandConfig,
    buf: Vec<char>,
    last: Option<LastExpansion>,
    /// Text to type back if the in-flight expansion is later undone
    /// (abbreviation + trailing). Set when `feed` returns `Expand`,
    /// finalised by `note_expanded`, dropped by `reset`.
    pending_restore: Option<String>,
}

impl AutoExpander {
    pub fn new(table: AbbrevTable, cfg: AutoExpandConfig) -> Self {
        AutoExpander {
            table,
            cfg,
            buf: Vec::with_capacity(BUFFER_CAP),
            last: None,
            pending_restore: None,
        }
    }

    /// Clear all transient state (buffer, undo record, pending expansion).
    /// Called on focus loss and when an expansion attempt is aborted.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.last = None;
        self.pending_restore = None;
    }

    pub fn set_table(&mut self, table: AbbrevTable) {
        self.table = table;
        self.reset();
    }

    pub fn set_config(&mut self, cfg: AutoExpandConfig) {
        self.cfg = cfg;
        self.reset();
    }

    pub fn config(&self) -> &AutoExpandConfig {
        &self.cfg
    }

    #[cfg(test)]
    fn buffer(&self) -> &[char] {
        &self.buf
    }

    fn push(&mut self, c: char) {
        self.buf.push(c);
        if self.buf.len() > BUFFER_CAP {
            let overflow = self.buf.len() - BUFFER_CAP;
            self.buf.drain(0..overflow);
        }
    }

    /// Take the undo record iff it is still fresh.
    fn take_fresh_undo(&mut self) -> Option<LastExpansion> {
        match self.last.take() {
            Some(l) if l.fresh => Some(l),
            _ => None,
        }
    }

    /// Stale the undo record after any non-undo event consumes the
    /// "immediately after" window.
    fn age_undo(&mut self) {
        if let Some(l) = self.last.as_mut() {
            l.fresh = false;
        }
    }

    /// Feed one decoded key event. Returns the action the executor should
    /// perform. Pure: no clipboard, no time, no FFI.
    ///
    /// The type buffer is maintained **regardless** of `cfg.enabled` so the
    /// on-demand hotkey path ([`match_buffer_now`]) always has the typed word
    /// available, even when *automatic* expansion (delimiter / immediate) is
    /// switched off. Auto-expansion actions only fire when `cfg.enabled`.
    pub fn feed(&mut self, ev: KeyEvent) -> ExpandAction {
        match ev {
            KeyEvent::Reset => {
                self.buf.clear();
                self.last = None;
                self.pending_restore = None;
                ExpandAction::None
            }
            KeyEvent::Backspace => {
                // Undo wins if a fresh expansion is pending (auto-mode only).
                if self.cfg.enabled && self.cfg.undo_enabled {
                    if let Some(l) = self.take_fresh_undo() {
                        self.reset();
                        return ExpandAction::Undo {
                            delete_chars: l.inserted_chars,
                            restore_text: l.restore_text,
                        };
                    }
                }
                self.buf.pop();
                ExpandAction::None
            }
            KeyEvent::Char(c) => {
                if self.cfg.enabled {
                    self.age_undo();
                }
                if is_delimiter(c) {
                    // A delimiter ends the current word: in delimiter auto-mode
                    // it may trigger an expansion; either way the buffer clears.
                    let action = if self.cfg.enabled && self.cfg.trigger == Trigger::Delimiter {
                        self.try_match(Some(c))
                    } else {
                        ExpandAction::None
                    };
                    self.buf.clear();
                    action
                } else {
                    self.push(c);
                    if self.cfg.enabled && self.cfg.trigger == Trigger::Immediate {
                        self.try_match(None)
                    } else {
                        ExpandAction::None
                    }
                }
            }
        }
    }

    /// On-demand match for the abbreviation **hotkey** (e.g. `Alt+1`): find the
    /// longest abbreviation that is a suffix of the buffer the user just typed,
    /// ignoring the auto-expansion trigger mode. Returns an `Expand` action
    /// (no trailing delimiter — the hotkey, not a delimiter, triggered it) or
    /// `None`. Because the buffer is tracked from observed keystrokes, this
    /// works **everywhere — including terminals** (it never reads the field).
    pub fn match_buffer_now(&mut self) -> ExpandAction {
        self.try_match(None)
    }

    /// Attempt a suffix match on the current buffer. `trailing` is the
    /// delimiter that should be re-emitted after the body (delimiter mode)
    /// or `None` (immediate mode).
    fn try_match(&mut self, trailing: Option<char>) -> ExpandAction {
        let Some((snippet_id, abbr_len)) =
            self.table
                .match_suffix(&self.buf, self.cfg.match_case, self.cfg.expand_inside_words)
        else {
            return ExpandAction::None;
        };
        // Capture the exact typed abbreviation (for undo) from the buffer tail.
        let mut restore: String = self.buf[self.buf.len() - abbr_len..].iter().collect();
        if let Some(t) = trailing {
            restore.push(t);
        }
        self.pending_restore = Some(restore);
        // Backspaces = abbreviation chars + the trailing delimiter (if any,
        // since it has already been typed into the field).
        let backspaces = abbr_len + usize::from(trailing.is_some());
        ExpandAction::Expand {
            snippet_id,
            backspaces,
            trailing,
        }
    }

    /// Called by the executor after a successful expansion so the engine can
    /// arm undo and reset its buffer. `inserted_chars` is the total number of
    /// characters actually inserted (rendered body + re-emitted trailing).
    pub fn note_expanded(&mut self, inserted_chars: usize) {
        let restore_text = self.pending_restore.take().unwrap_or_default();
        self.buf.clear();
        self.last = if self.cfg.undo_enabled {
            Some(LastExpansion {
                restore_text,
                inserted_chars,
                fresh: true,
            })
        } else {
            None
        };
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Runtime — Tauri state, lifecycle, platform key monitor
// ─────────────────────────────────────────────────────────────────────────

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tauri::Manager;

/// Managed Tauri state: the live engine behind a mutex. The platform key
/// monitor reaches the same `Arc` through the global [`RT`] context.
pub struct AutoExpandState {
    pub engine: Arc<Mutex<AutoExpander>>,
}

impl Default for AutoExpandState {
    fn default() -> Self {
        AutoExpandState {
            engine: Arc::new(Mutex::new(AutoExpander::new(
                AbbrevTable::default(),
                AutoExpandConfig::default(),
            ))),
        }
    }
}

/// Process-wide context the `extern "C"` / `extern "system"` monitor
/// callbacks need (they can't capture a closure environment). Set once on the
/// first `start`.
struct Runtime {
    app: tauri::AppHandle,
    db: DbHandle,
    engine: Arc<Mutex<AutoExpander>>,
}

static RT: OnceLock<Runtime> = OnceLock::new();
/// True while we are synthesising our own keystrokes (backspaces + paste +
/// trailing) — the monitor ignores events during this window so an expansion
/// can't recursively trigger itself. On Windows the injected-event flag is
/// the primary guard; this is the macOS guard + a cross-platform backstop.
static INJECTING: AtomicBool = AtomicBool::new(false);
/// Whether the passive monitor is currently armed.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// What a deferred injection should do.
enum InjectCmd {
    Expand {
        snippet_id: i64,
        backspaces: usize,
        trailing: Option<char>,
    },
    Undo {
        delete_chars: usize,
        restore_text: String,
    },
}

/// Core dispatch shared by both platform monitors. Returns `true` if the
/// triggering event should be **consumed** (dropped) — only true for the
/// Backspace that triggers an undo. Cheap: a buffer feed + maybe scheduling
/// a deferred injection; the slow safety checks happen in `do_inject`.
fn on_event(ev: KeyEvent) -> bool {
    if INJECTING.load(Ordering::SeqCst) || !RUNNING.load(Ordering::SeqCst) {
        return false;
    }
    let Some(rt) = RT.get() else {
        return false;
    };
    let action = rt.engine.lock().feed(ev);
    match action {
        ExpandAction::None => false,
        ExpandAction::Expand {
            snippet_id,
            backspaces,
            trailing,
        } => {
            INJECTING.store(true, Ordering::SeqCst);
            schedule_injection(InjectCmd::Expand {
                snippet_id,
                backspaces,
                trailing,
            });
            false
        }
        ExpandAction::Undo {
            delete_chars,
            restore_text,
        } => {
            INJECTING.store(true, Ordering::SeqCst);
            schedule_injection(InjectCmd::Undo {
                delete_chars,
                restore_text,
            });
            true
        }
    }
}

/// Schedule the actual keystroke synthesis. On macOS it must run on the main
/// thread (TSM); the tap callback is already on the main run loop, so we
/// defer via `run_on_main_thread` (lets the triggering event finish delivery
/// first). Elsewhere a worker thread is fine.
fn schedule_injection(cmd: InjectCmd) {
    let Some(rt) = RT.get() else {
        INJECTING.store(false, Ordering::SeqCst);
        return;
    };
    #[cfg(target_os = "macos")]
    {
        let _ = rt.app.run_on_main_thread(move || do_inject(cmd));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = rt;
        std::thread::spawn(move || do_inject(cmd));
    }
}

/// Clear the injection guard after a short grace window so the trailing
/// synthesized events (which deliver after the posting returns) are still
/// ignored by the monitor.
fn clear_injecting_after_grace() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(150));
        INJECTING.store(false, Ordering::SeqCst);
    });
}

/// Run the deferred injection. All the *slow* safety checks live here (not in
/// the hot monitor callback): refuse to expand into our own popup, a secure
/// event-input state, or a password field.
fn do_inject(cmd: InjectCmd) {
    let Some(rt) = RT.get() else {
        INJECTING.store(false, Ordering::SeqCst);
        return;
    };
    match cmd {
        InjectCmd::Expand {
            snippet_id,
            backspaces,
            trailing,
        } => {
            if !pre_inject_ok() {
                rt.engine.lock().reset();
                INJECTING.store(false, Ordering::SeqCst);
                return;
            }
            let watcher = rt.app.try_state::<crate::clipboard_watcher::WatcherState>();
            let res = crate::expander::auto_expand_inject(
                &rt.db,
                snippet_id,
                backspaces,
                trailing,
                watcher.as_deref(),
            );
            match res {
                Ok(inserted) => rt.engine.lock().note_expanded(inserted),
                Err(e) => {
                    tracing::warn!("auto_expand: injection failed: {e:#}");
                    rt.engine.lock().reset();
                }
            }
        }
        InjectCmd::Undo {
            delete_chars,
            restore_text,
        } => {
            if let Err(e) = crate::expander::auto_undo_inject(delete_chars, &restore_text) {
                tracing::warn!("auto_expand: undo failed: {e:#}");
            }
        }
    }
    clear_injecting_after_grace();
}

/// Slow pre-injection safety gate (see `do_inject`).
fn pre_inject_ok() -> bool {
    if crate::expander::inspector_rust_is_frontmost_public() {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        if !crate::expander::accessibility_granted()
            || crate::expander::secure_event_input_active()
        {
            return false;
        }
    }
    !matches!(
        crate::text_field::default_field_access().is_focused_field_secure(),
        Ok(true)
    )
}

/// On-demand expansion for the abbreviation **hotkey** (`Alt+1`), backed by
/// the keystroke buffer. Returns `true` if it expanded the word the user just
/// typed; `false` if the monitor isn't tracking, no abbreviation matched, or a
/// safety gate blocked it — in which case the caller falls back to the legacy
/// AX/UIA read path. Works in any app (incl. terminals) because it never reads
/// the focused field.
///
/// Must run on the main thread on macOS (synthesizes keystrokes via enigo);
/// the expander hotkey handler already dispatches onto the main thread.
pub fn try_hotkey_expand() -> bool {
    if INJECTING.load(Ordering::SeqCst) {
        return false;
    }
    let Some(rt) = RT.get() else {
        return false;
    };
    if !RUNNING.load(Ordering::SeqCst) {
        tracing::info!("auto_expand: hotkey — passive monitor not tracking (RUNNING=false)");
        return false; // monitor not tracking → no buffer to expand from
    }
    let action = rt.engine.lock().match_buffer_now();
    let ExpandAction::Expand {
        snippet_id,
        backspaces,
        trailing,
    } = action
    else {
        tracing::info!("auto_expand: hotkey — no abbreviation matched in the tracked buffer");
        return false;
    };
    tracing::info!(snippet_id, backspaces, "auto_expand: hotkey expanding from buffer");
    if !pre_inject_ok() {
        // A password field / our own popup etc. — don't expand here; let the
        // caller decide (the AX path applies its own, identical guards).
        rt.engine.lock().reset();
        return false;
    }
    INJECTING.store(true, Ordering::SeqCst);
    let watcher = rt.app.try_state::<crate::clipboard_watcher::WatcherState>();
    let result =
        crate::expander::auto_expand_inject(&rt.db, snippet_id, backspaces, trailing, watcher.as_deref());
    match result {
        Ok(inserted) => {
            rt.engine.lock().note_expanded(inserted);
            clear_injecting_after_grace();
            true
        }
        Err(e) => {
            tracing::warn!("auto_expand: hotkey expansion failed: {e:#}");
            rt.engine.lock().reset();
            INJECTING.store(false, Ordering::SeqCst);
            false
        }
    }
}

/// (Re)load config + abbreviation table from the DB into the engine. Safe to
/// call whenever snippets or the config change.
pub fn refresh(db: &DbHandle, state: &AutoExpandState) {
    let cfg = load_config(db);
    let table = AbbrevTable::from_db(db);
    tracing::debug!("auto_expand: {} abbreviations indexed", table.len());
    let mut eng = state.engine.lock();
    eng.set_config(cfg);
    eng.set_table(table);
}

/// Rebuild only the abbreviation table (e.g. after a snippet CRUD op),
/// preserving the running config.
pub fn rebuild_table(db: &DbHandle, state: &AutoExpandState) {
    let table = AbbrevTable::from_db(db);
    state.engine.lock().set_table(table);
}

/// Start or stop the passive monitor to match the current config. Installs
/// the platform key monitor on first enable (idempotent — the OS tap/hook is
/// installed once and re-armed via the `RUNNING` flag thereafter).
pub fn apply(app: &tauri::AppHandle, db: &DbHandle, state: &AutoExpandState) {
    // Populate the global runtime context once.
    let _ = RT.set(Runtime {
        app: app.clone(),
        db: db.clone(),
        engine: state.engine.clone(),
    });
    refresh(db, state);

    // Arm the keystroke monitor when EITHER automatic auto-expansion is on OR
    // the abbreviation hotkey is enabled. In the hotkey-only case the engine's
    // `cfg.enabled` is false, so the monitor merely *tracks* keystrokes (no
    // automatic expansion) — that buffer is what `try_hotkey_expand` uses so
    // the hotkey works everywhere, including terminals.
    let auto_enabled = state.engine.lock().config().enabled;
    let hotkey_enabled =
        crate::settings::get_bool(db, crate::expander::KEY_ENABLED, false).unwrap_or(false);
    let want_monitor = auto_enabled || hotkey_enabled;
    if want_monitor {
        // On macOS we need Accessibility to read/write keystrokes; without it
        // installing the tap is pointless. Mirror the expander's behaviour:
        // skip silently (the Settings banner surfaces the permission need).
        #[cfg(target_os = "macos")]
        if !crate::expander::accessibility_granted() {
            tracing::info!(
                "auto_expand: enabled but Accessibility not granted — monitor not installed yet"
            );
            RUNNING.store(false, Ordering::SeqCst);
            return;
        }
        match platform::install() {
            Ok(()) => {
                RUNNING.store(true, Ordering::SeqCst);
                tracing::info!("auto_expand: passive monitor armed");
            }
            Err(e) => tracing::warn!("auto_expand: monitor install failed: {e:#}"),
        }
    } else {
        RUNNING.store(false, Ordering::SeqCst);
        platform::set_enabled(false);
    }
}

// ── Platform key monitor ──────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::ffi::c_void;

    type CGEventRef = *mut c_void;
    type CGEventTapProxy = *mut c_void;
    type CFMachPortRef = *mut c_void;
    type CFAllocatorRef = *mut c_void;
    type CFRunLoopRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFStringRef = *const c_void;
    type CGEventMask = u64;
    type CGEventTapCallBack = extern "C" fn(
        proxy: CGEventTapProxy,
        event_type: u32,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef;

    const SESSION_EVENT_TAP: u32 = 1;
    const HEAD_INSERT_EVENT_TAP: u32 = 0;
    const EVENT_TAP_OPTION_DEFAULT: u32 = 0; // active tap (can drop events)
    const EVT_KEY_DOWN: u32 = 10;
    const EVT_LEFT_MOUSE_DOWN: u32 = 1;
    const EVT_RIGHT_MOUSE_DOWN: u32 = 3;
    const EVT_TAP_DISABLED_TIMEOUT: u32 = 0xFFFFFFFE;
    const EVT_TAP_DISABLED_USER_INPUT: u32 = 0xFFFFFFFF;
    const KEYCODE_FIELD: u32 = 9;
    // Modifier flags we treat as "this is a shortcut, not text" → reset.
    const FLAG_MASK_COMMAND: u64 = 0x0010_0000;
    const FLAG_MASK_CONTROL: u64 = 0x0004_0000;
    // Option/Alt — the abbreviation hotkey (default `Alt+1`) uses it. We must
    // leave the tracked buffer UNTOUCHED for these (not reset, not append), so
    // the hotkey's own keypress doesn't clobber the abbreviation `try_hotkey_expand`
    // is about to expand from the buffer.
    const FLAG_MASK_ALTERNATE: u64 = 0x0008_0000;

    static TAP_PORT: OnceLock<usize> = OnceLock::new(); // CFMachPortRef as usize
    static TAP_INSTALLED: AtomicBool = AtomicBool::new(false);

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: CGEventMask,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        fn CGEventGetFlags(event: CGEventRef) -> u64;
        fn CGEventKeyboardGetUnicodeString(
            event: CGEventRef,
            max_len: usize,
            actual_len: *mut usize,
            buffer: *mut u16,
        );
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFMachPortCreateRunLoopSource(
            allocator: CFAllocatorRef,
            port: CFMachPortRef,
            order: isize,
        ) -> CFRunLoopSourceRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopGetMain() -> CFRunLoopRef;
        static kCFRunLoopCommonModes: CFStringRef;
    }

    /// Decode the typed character from a keyDown event, honouring layout +
    /// shift. Returns `None` for non-text producing keys.
    fn decode_char(event: CGEventRef) -> Option<char> {
        let mut buf = [0u16; 8];
        let mut len: usize = 0;
        unsafe {
            CGEventKeyboardGetUnicodeString(event, buf.len(), &mut len, buf.as_mut_ptr());
        }
        if len == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(&buf[..len]);
        let mut chars = s.chars();
        let c = chars.next()?;
        // Reject control chars (Backspace, Enter as \r, Tab handled
        // separately as Reset / delimiter). Accept space + printable.
        if c.is_control() {
            None
        } else {
            Some(c)
        }
    }

    /// macOS keycodes that should reset the buffer (navigation / editing).
    fn is_reset_keycode(kc: i64) -> bool {
        matches!(
            kc,
            123 | 124 | 125 | 126 // arrows
                | 115 | 119          // home / end
                | 116 | 121          // page up / down
                | 53                 // escape
                | 36 | 76            // return / keypad-enter (navigation)
                | 48                 // tab
                | 117                // forward-delete
        )
    }

    const KEYCODE_DELETE: i64 = 51; // Backspace

    extern "C" fn tap_callback(
        _proxy: CGEventTapProxy,
        event_type: u32,
        event: CGEventRef,
        _user_info: *mut c_void,
    ) -> CGEventRef {
        // The OS disables a tap that times out or is disabled by user input;
        // re-enable it so we keep receiving events (same as input_lock).
        if event_type == EVT_TAP_DISABLED_TIMEOUT || event_type == EVT_TAP_DISABLED_USER_INPUT {
            if let Some(&p) = TAP_PORT.get() {
                unsafe { CGEventTapEnable(p as CFMachPortRef, true) };
            }
            return event;
        }

        let consume = match event_type {
            EVT_LEFT_MOUSE_DOWN | EVT_RIGHT_MOUSE_DOWN => {
                on_event(KeyEvent::Reset);
                false
            }
            EVT_KEY_DOWN => {
                let kc = unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) };
                let flags = unsafe { CGEventGetFlags(event) };
                if (flags & FLAG_MASK_ALTERNATE) != 0 {
                    // Option/Alt-modified key — this is (or could be) the
                    // abbreviation hotkey (default Alt+1). Leave the buffer
                    // exactly as it is so `try_hotkey_expand` can still match
                    // the abbreviation the user just typed; don't append the
                    // hotkey char and don't reset. Pass the event through.
                    false
                } else if (flags & (FLAG_MASK_COMMAND | FLAG_MASK_CONTROL)) != 0 {
                    // A shortcut (Cmd/Ctrl+key) — not text; reset.
                    on_event(KeyEvent::Reset)
                } else if kc == KEYCODE_DELETE {
                    on_event(KeyEvent::Backspace)
                } else if is_reset_keycode(kc) {
                    on_event(KeyEvent::Reset)
                } else if let Some(c) = decode_char(event) {
                    on_event(KeyEvent::Char(c))
                } else {
                    on_event(KeyEvent::Reset)
                }
            }
            _ => false,
        };

        if consume {
            std::ptr::null_mut()
        } else {
            event
        }
    }

    pub fn install() -> anyhow::Result<()> {
        if TAP_INSTALLED.load(Ordering::SeqCst) {
            // Already installed — just re-enable it.
            if let Some(&p) = TAP_PORT.get() {
                unsafe { CGEventTapEnable(p as CFMachPortRef, true) };
            }
            return Ok(());
        }
        let mask: CGEventMask = (1u64 << EVT_KEY_DOWN)
            | (1u64 << EVT_LEFT_MOUSE_DOWN)
            | (1u64 << EVT_RIGHT_MOUSE_DOWN);
        let tap_port = unsafe {
            CGEventTapCreate(
                SESSION_EVENT_TAP,
                HEAD_INSERT_EVENT_TAP,
                EVENT_TAP_OPTION_DEFAULT,
                mask,
                tap_callback,
                std::ptr::null_mut(),
            )
        };
        if tap_port.is_null() {
            return Err(anyhow::anyhow!(
                "CGEventTapCreate returned NULL — grant Inspector Rust Accessibility access."
            ));
        }
        unsafe {
            let src = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap_port, 0);
            if src.is_null() {
                return Err(anyhow::anyhow!("CFMachPortCreateRunLoopSource returned NULL"));
            }
            CFRunLoopAddSource(CFRunLoopGetMain(), src, kCFRunLoopCommonModes);
            CGEventTapEnable(tap_port, true);
        }
        let _ = TAP_PORT.set(tap_port as usize);
        TAP_INSTALLED.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn set_enabled(enable: bool) {
        if let Some(&p) = TAP_PORT.get() {
            unsafe { CGEventTapEnable(p as CFMachPortRef, enable) };
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::sync::atomic::AtomicIsize;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, GetKeyboardState, ToUnicodeEx, VK_BACK, VK_DELETE, VK_DOWN, VK_END,
        VK_ESCAPE, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_TAB, VK_UP,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK,
        KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
    };

    static HOOK: AtomicIsize = AtomicIsize::new(0);
    static THREAD_STARTED: AtomicBool = AtomicBool::new(false);

    fn vk_is_reset(vk: u16) -> bool {
        let r = |k: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| k.0 == vk;
        r(VK_LEFT)
            || r(VK_RIGHT)
            || r(VK_UP)
            || r(VK_DOWN)
            || r(VK_HOME)
            || r(VK_END)
            || r(VK_PRIOR)
            || r(VK_NEXT)
            || r(VK_ESCAPE)
            || r(VK_RETURN)
            || r(VK_TAB)
            || r(VK_DELETE)
    }

    /// Decode the character for a virtual-key using the current layout +
    /// keyboard state (shift/caps). Returns `None` for non-text keys.
    fn decode_char(vk: u32, scan: u32) -> Option<char> {
        unsafe {
            let mut state = [0u8; 256];
            if GetKeyboardState(&mut state).is_err() {
                return None;
            }
            let layout = GetKeyboardLayout(0);
            let mut buf = [0u16; 8];
            let n = ToUnicodeEx(vk, scan, &state, &mut buf, 0, Some(layout));
            if n == 1 {
                let c = char::from_u32(buf[0] as u32)?;
                if c.is_control() {
                    None
                } else {
                    Some(c)
                }
            } else {
                None
            }
        }
    }

    extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && (wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN) {
            let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            // Ignore our own synthesized events.
            let injected = (kb.flags.0 & LLKHF_INJECTED.0) != 0;
            if !injected {
                let vk = kb.vkCode as u16;
                let consume = if vk == VK_BACK.0 {
                    on_event(KeyEvent::Backspace)
                } else if vk_is_reset(vk) {
                    on_event(KeyEvent::Reset)
                } else if let Some(c) = decode_char(kb.vkCode, kb.scanCode) {
                    on_event(KeyEvent::Char(c))
                } else {
                    on_event(KeyEvent::Reset)
                };
                if consume {
                    return LRESULT(1);
                }
            }
        }
        unsafe {
            CallNextHookEx(
                Some(HHOOK(HOOK.load(Ordering::SeqCst) as *mut _)),
                code,
                wparam,
                lparam,
            )
        }
    }

    pub fn install() -> anyhow::Result<()> {
        // The low-level keyboard hook needs a thread with a message loop. Spawn
        // it once; the loop keeps the hook alive. Re-enable is handled by the
        // RUNNING flag in `on_event` (the hook stays installed but no-ops when
        // RUNNING is false).
        //
        // A live hook already exists → nothing to do.
        if HOOK.load(Ordering::SeqCst) != 0 {
            return Ok(());
        }
        // Guard against a double spawn. CRUCIALLY: if `SetWindowsHookExW` fails
        // (low privilege, AV interference, …) the thread resets this flag so a
        // later `install()` — e.g. after the blocking condition clears — retries
        // instead of silently no-op'ing forever.
        if THREAD_STARTED.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        std::thread::spawn(|| unsafe {
            let h = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0);
            match h {
                Ok(hook) => {
                    HOOK.store(hook.0 as isize, Ordering::SeqCst);
                    let mut msg = MSG::default();
                    // Pump messages so the LL hook keeps receiving events.
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
                    let _ = UnhookWindowsHookEx(hook);
                    HOOK.store(0, Ordering::SeqCst);
                    THREAD_STARTED.store(false, Ordering::SeqCst);
                }
                Err(e) => {
                    tracing::warn!("auto_expand: SetWindowsHookExW failed: {e:?}");
                    THREAD_STARTED.store(false, Ordering::SeqCst);
                }
            }
        });
        Ok(())
    }

    pub fn set_enabled(_enable: bool) {
        // The RUNNING flag gates `on_event`; the hook itself stays installed.
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    /// Linux has no rootless system-wide key tap under Wayland; the
    /// clipboard-paste expander fallback remains the path there. Auto-mode is
    /// a no-op (best-effort, documented).
    pub fn install() -> anyhow::Result<()> {
        Err(anyhow::anyhow!(
            "passive auto-expansion is not available on this platform yet"
        ))
    }
    pub fn set_enabled(_enable: bool) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> AbbrevTable {
        AbbrevTable::from_pairs(vec![("mfg", 1), ("addr", 2), ("ad", 3), ("ä", 4), ("brb", 5)])
    }

    fn engine(trigger: Trigger) -> AutoExpander {
        let cfg = AutoExpandConfig {
            enabled: true,
            trigger,
            ..Default::default()
        };
        AutoExpander::new(table(), cfg)
    }

    fn type_chars(e: &mut AutoExpander, s: &str) -> Vec<ExpandAction> {
        s.chars().map(|c| e.feed(KeyEvent::Char(c))).collect()
    }

    #[test]
    fn delimiter_mode_expands_on_space() {
        let mut e = engine(Trigger::Delimiter);
        // type "mfg" — no expansion yet
        for c in "mfg".chars() {
            assert_eq!(e.feed(KeyEvent::Char(c)), ExpandAction::None);
        }
        // space triggers
        let a = e.feed(KeyEvent::Char(' '));
        assert_eq!(
            a,
            ExpandAction::Expand {
                snippet_id: 1,
                backspaces: 4, // "mfg" + the space
                trailing: Some(' '),
            }
        );
    }

    #[test]
    fn delimiter_mode_does_not_expand_without_delimiter() {
        let mut e = engine(Trigger::Delimiter);
        let actions = type_chars(&mut e, "mfg");
        assert!(actions.iter().all(|a| *a == ExpandAction::None));
    }

    #[test]
    fn immediate_mode_expands_on_completion() {
        let mut e = engine(Trigger::Immediate);
        assert_eq!(e.feed(KeyEvent::Char('m')), ExpandAction::None);
        assert_eq!(e.feed(KeyEvent::Char('f')), ExpandAction::None);
        let a = e.feed(KeyEvent::Char('g'));
        assert_eq!(
            a,
            ExpandAction::Expand {
                snippet_id: 1,
                backspaces: 3,
                trailing: None,
            }
        );
    }

    #[test]
    fn longest_abbreviation_wins() {
        // "addr" and "ad" both exist; typing "addr" + space must pick "addr".
        let mut e = engine(Trigger::Delimiter);
        for c in "addr".chars() {
            e.feed(KeyEvent::Char(c));
        }
        let a = e.feed(KeyEvent::Char(' '));
        match a {
            ExpandAction::Expand { snippet_id, backspaces, .. } => {
                assert_eq!(snippet_id, 2);
                assert_eq!(backspaces, 5);
            }
            _ => panic!("expected expand, got {a:?}"),
        }
    }

    #[test]
    fn shorter_abbreviation_still_matches_alone() {
        let mut e = engine(Trigger::Delimiter);
        e.feed(KeyEvent::Char('a'));
        e.feed(KeyEvent::Char('d'));
        let a = e.feed(KeyEvent::Char(' '));
        match a {
            ExpandAction::Expand { snippet_id, .. } => assert_eq!(snippet_id, 3),
            _ => panic!("expected expand for 'ad'"),
        }
    }

    #[test]
    fn word_boundary_blocks_glued_match_by_default() {
        // "xmfg " — "mfg" is glued to "x"; default inside_words=false → no fire.
        let mut e = engine(Trigger::Delimiter);
        for c in "xmfg".chars() {
            e.feed(KeyEvent::Char(c));
        }
        assert_eq!(e.feed(KeyEvent::Char(' ')), ExpandAction::None);
    }

    #[test]
    fn inside_words_allows_glued_match() {
        let mut cfg = AutoExpandConfig {
            enabled: true,
            trigger: Trigger::Delimiter,
            expand_inside_words: true,
            ..Default::default()
        };
        cfg.expand_inside_words = true;
        let mut e = AutoExpander::new(table(), cfg);
        for c in "xmfg".chars() {
            e.feed(KeyEvent::Char(c));
        }
        match e.feed(KeyEvent::Char(' ')) {
            ExpandAction::Expand { snippet_id, .. } => assert_eq!(snippet_id, 1),
            other => panic!("expected expand, got {other:?}"),
        }
    }

    #[test]
    fn reset_clears_buffer() {
        let mut e = engine(Trigger::Immediate);
        e.feed(KeyEvent::Char('m'));
        e.feed(KeyEvent::Char('f'));
        e.feed(KeyEvent::Reset);
        // Typing 'g' alone now does not complete "mfg".
        assert_eq!(e.feed(KeyEvent::Char('g')), ExpandAction::None);
    }

    #[test]
    fn backspace_pops_buffer() {
        let mut e = engine(Trigger::Immediate);
        e.feed(KeyEvent::Char('m'));
        e.feed(KeyEvent::Char('f'));
        e.feed(KeyEvent::Char('x'));
        e.feed(KeyEvent::Backspace); // remove 'x'
        // buffer is "mf"; typing 'g' completes "mfg"
        match e.feed(KeyEvent::Char('g')) {
            ExpandAction::Expand { snippet_id, .. } => assert_eq!(snippet_id, 1),
            other => panic!("expected expand, got {other:?}"),
        }
    }

    #[test]
    fn multibyte_abbreviation_counts_one_backspace() {
        // "ä" is a single char abbreviation → one backspace, not two.
        let mut e = engine(Trigger::Immediate);
        match e.feed(KeyEvent::Char('ä')) {
            ExpandAction::Expand { snippet_id, backspaces, .. } => {
                assert_eq!(snippet_id, 4);
                assert_eq!(backspaces, 1);
            }
            other => panic!("expected expand, got {other:?}"),
        }
    }

    #[test]
    fn case_insensitive_by_default() {
        let mut e = engine(Trigger::Delimiter);
        for c in "MFG".chars() {
            e.feed(KeyEvent::Char(c));
        }
        match e.feed(KeyEvent::Char(' ')) {
            ExpandAction::Expand { snippet_id, .. } => assert_eq!(snippet_id, 1),
            other => panic!("expected expand, got {other:?}"),
        }
    }

    #[test]
    fn case_sensitive_blocks_wrong_case() {
        let cfg = AutoExpandConfig {
            enabled: true,
            trigger: Trigger::Delimiter,
            match_case: true,
            ..Default::default()
        };
        let mut e = AutoExpander::new(table(), cfg);
        for c in "MFG".chars() {
            e.feed(KeyEvent::Char(c));
        }
        assert_eq!(e.feed(KeyEvent::Char(' ')), ExpandAction::None);
    }

    #[test]
    fn disabled_engine_never_auto_expands() {
        let cfg = AutoExpandConfig {
            enabled: false,
            trigger: Trigger::Immediate,
            ..Default::default()
        };
        let mut e = AutoExpander::new(table(), cfg);
        for c in "mfg".chars() {
            assert_eq!(e.feed(KeyEvent::Char(c)), ExpandAction::None);
        }
    }

    #[test]
    fn buffer_tracks_even_when_disabled_so_hotkey_can_expand() {
        // The abbreviation-hotkey path relies on the buffer being maintained
        // even when automatic expansion is off. Type the word, then the
        // on-demand match (what Alt+1 calls) finds it regardless of trigger.
        let cfg = AutoExpandConfig {
            enabled: false,
            trigger: Trigger::Delimiter,
            ..Default::default()
        };
        let mut e = AutoExpander::new(table(), cfg);
        for c in "mfg".chars() {
            assert_eq!(e.feed(KeyEvent::Char(c)), ExpandAction::None);
        }
        assert_eq!(
            e.match_buffer_now(),
            ExpandAction::Expand {
                snippet_id: 1,
                backspaces: 3, // abbreviation only — no trailing delimiter
                trailing: None,
            }
        );
    }

    #[test]
    fn match_buffer_now_ignores_trigger_and_needs_no_delimiter() {
        // Even in delimiter auto-mode, the hotkey expands mid-word (no space).
        let mut e = engine(Trigger::Delimiter);
        for c in "addr".chars() {
            assert_eq!(e.feed(KeyEvent::Char(c)), ExpandAction::None);
        }
        match e.match_buffer_now() {
            ExpandAction::Expand { snippet_id, backspaces, trailing } => {
                assert_eq!(snippet_id, 2);
                assert_eq!(backspaces, 4);
                assert_eq!(trailing, None);
            }
            other => panic!("expected expand, got {other:?}"),
        }
    }

    #[test]
    fn match_buffer_now_returns_none_without_a_match() {
        let mut e = engine(Trigger::Delimiter);
        for c in "zzz".chars() {
            e.feed(KeyEvent::Char(c));
        }
        assert_eq!(e.match_buffer_now(), ExpandAction::None);
    }

    #[test]
    fn delimiter_clears_buffer_even_when_disabled() {
        // So the hotkey expands the *current* word, not one before a space.
        let cfg = AutoExpandConfig {
            enabled: false,
            trigger: Trigger::Delimiter,
            ..Default::default()
        };
        let mut e = AutoExpander::new(table(), cfg);
        for c in "mfg ".chars() {
            e.feed(KeyEvent::Char(c));
        }
        // After the space the buffer is empty → no stale match.
        assert_eq!(e.match_buffer_now(), ExpandAction::None);
    }

    #[test]
    fn undo_fires_on_backspace_right_after_expansion() {
        let mut e = engine(Trigger::Immediate);
        // Expand "brb".
        e.feed(KeyEvent::Char('b'));
        e.feed(KeyEvent::Char('r'));
        let a = e.feed(KeyEvent::Char('b'));
        assert!(matches!(a, ExpandAction::Expand { snippet_id: 5, .. }));
        // Executor reports back the expansion (body "be right back" = 12 chars).
        e.note_expanded(12);
        // The very next Backspace undoes it.
        match e.feed(KeyEvent::Backspace) {
            ExpandAction::Undo { delete_chars, restore_text } => {
                assert_eq!(delete_chars, 12);
                assert_eq!(restore_text, "brb");
            }
            other => panic!("expected undo, got {other:?}"),
        }
    }

    #[test]
    fn undo_restores_abbreviation_plus_trailing_delimiter() {
        // Delimiter mode: "brb" + space expands; undo must restore "brb ".
        let mut e = engine(Trigger::Delimiter);
        for c in "brb".chars() {
            e.feed(KeyEvent::Char(c));
        }
        let a = e.feed(KeyEvent::Char(' '));
        assert!(matches!(a, ExpandAction::Expand { snippet_id: 5, .. }));
        // Inserted = body (12) + the re-emitted space (1) = 13.
        e.note_expanded(13);
        match e.feed(KeyEvent::Backspace) {
            ExpandAction::Undo { delete_chars, restore_text } => {
                assert_eq!(delete_chars, 13);
                assert_eq!(restore_text, "brb ");
            }
            other => panic!("expected undo, got {other:?}"),
        }
    }

    #[test]
    fn undo_window_is_only_immediately_after() {
        let mut e = engine(Trigger::Immediate);
        e.feed(KeyEvent::Char('b'));
        e.feed(KeyEvent::Char('r'));
        e.feed(KeyEvent::Char('b'));
        e.note_expanded(12);
        // A non-backspace char ages the undo window.
        e.feed(KeyEvent::Char('x'));
        assert_eq!(e.feed(KeyEvent::Backspace), ExpandAction::None);
    }

    #[test]
    fn undo_disabled_does_not_arm() {
        let cfg = AutoExpandConfig {
            enabled: true,
            trigger: Trigger::Immediate,
            undo_enabled: false,
            ..Default::default()
        };
        let mut e = AutoExpander::new(table(), cfg);
        e.feed(KeyEvent::Char('b'));
        e.feed(KeyEvent::Char('r'));
        e.feed(KeyEvent::Char('b'));
        e.note_expanded(12);
        assert_eq!(e.feed(KeyEvent::Backspace), ExpandAction::None);
    }

    #[test]
    fn buffer_is_capped() {
        let mut e = engine(Trigger::Delimiter);
        for _ in 0..(BUFFER_CAP + 50) {
            e.feed(KeyEvent::Char('z'));
        }
        assert!(e.buffer().len() <= BUFFER_CAP);
    }

    #[test]
    fn table_sorts_longest_first_and_matches() {
        let t = table();
        assert!(t.len() >= 5);
        // "addr" (4) sorts before "ad" (2).
        let buf: Vec<char> = "addr".chars().collect();
        assert_eq!(t.match_suffix(&buf, false, false), Some((2, 4)));
    }

    #[test]
    fn table_skips_empty_abbreviations() {
        let t = AbbrevTable::from_pairs(vec![("", 1), ("ok", 2)]);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn is_delimiter_classifies_common_chars() {
        assert!(is_delimiter(' '));
        assert!(is_delimiter('\n'));
        assert!(is_delimiter('.'));
        assert!(is_delimiter(','));
        assert!(!is_delimiter('a'));
        assert!(!is_delimiter('ä'));
        assert!(!is_delimiter('1'));
    }

    #[test]
    fn trigger_parses_loosely() {
        assert_eq!(Trigger::from_str_loose("immediate"), Trigger::Immediate);
        assert_eq!(Trigger::from_str_loose("Immediate "), Trigger::Immediate);
        assert_eq!(Trigger::from_str_loose("delimiter"), Trigger::Delimiter);
        assert_eq!(Trigger::from_str_loose("garbage"), Trigger::Delimiter);
    }

    #[test]
    fn config_round_trips_through_settings() {
        use parking_lot::Mutex;
        use rusqlite::Connection;
        use std::sync::Arc;
        let db = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        crate::settings::init_table(&db).unwrap();
        let cfg = AutoExpandConfig {
            enabled: true,
            trigger: Trigger::Immediate,
            match_case: true,
            expand_inside_words: true,
            undo_enabled: false,
        };
        save_config(&db, &cfg).unwrap();
        assert_eq!(load_config(&db), cfg);
    }

    #[test]
    fn default_config_is_conservative() {
        let d = AutoExpandConfig::default();
        assert!(!d.enabled);
        assert_eq!(d.trigger, Trigger::Delimiter);
        assert!(!d.match_case);
        assert!(!d.expand_inside_words);
        assert!(d.undo_enabled);
    }
}
