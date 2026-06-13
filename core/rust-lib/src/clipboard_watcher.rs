use anyhow::Result;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use clipboard_rs::common::RustImage;
use clipboard_rs::{
    Clipboard, ClipboardContext, ClipboardHandler, ClipboardWatcher, ClipboardWatcherContext,
    ContentFormat,
};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::db::{hash_payload, upsert_clip, DbHandle};
use crate::models::{ContentType, NewClip, MAX_IMAGE_BYTES};

/// Privacy setting keys (v0.76.0).
pub const KEY_EXCLUDE_APPS: &str = "clipboard.exclude_apps";
pub const KEY_AUTO_CLEAR_SECS: &str = "clipboard.auto_clear_seconds";

/// Whether `frontmost` matches any entry in the comma/newline-separated
/// `exclude_list` (case-insensitive substring). Pure + unit-tested.
pub fn is_excluded_app(frontmost: &str, exclude_list: &str) -> bool {
    let front = frontmost.to_lowercase();
    exclude_list
        .split([',', '\n'])
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .any(|pat| front.contains(&pat))
}

/// `Clone` is a cheap `Arc::clone` of both fields — used by the
/// expander's background-restore thread (v0.35.0+) to ferry a handle
/// past a thread boundary without lifetimes.
#[derive(Clone)]
pub struct WatcherState {
    pub paused: Arc<AtomicBool>,
    /// SHA-256 hash of a clipboard payload **we** wrote to the OS just
    /// now (typically a paste action). The watcher consumes-and-skips one
    /// matching event so plain-text-paste of an HTML clip doesn't create
    /// a duplicate "Text" history entry. Behaves like a one-shot fuse:
    /// each `mark_self_write()` arms it; the next watcher event matching
    /// the hash clears it.
    pub self_written: Arc<Mutex<Option<String>>>,
}

impl WatcherState {
    pub fn new() -> Self {
        Self {
            paused: Arc::new(AtomicBool::new(false)),
            self_written: Arc::new(Mutex::new(None)),
        }
    }

    /// Arm the self-write fuse with the hash of the payload we're about
    /// to put on the clipboard. The next clipboard-watcher event that
    /// hashes to the same value will be skipped + the fuse cleared.
    pub fn mark_self_write(&self, content_type: ContentType, content_data: &str) {
        let hash = hash_payload(content_type, content_data);
        *self.self_written.lock() = Some(hash);
    }
}

struct Handler {
    ctx: ClipboardContext,
    db: DbHandle,
    app: AppHandle,
    paused: Arc<AtomicBool>,
    self_written: Arc<Mutex<Option<String>>>,
    /// Bumped on every captured clip; an auto-clear timer only fires if the
    /// generation it recorded is still current (no newer copy happened).
    clear_gen: Arc<AtomicU64>,
}

impl ClipboardHandler for Handler {
    fn on_clipboard_change(&mut self) {
        if self.paused.load(Ordering::Relaxed) {
            return;
        }
        if let Err(e) = self.capture() {
            tracing::warn!("clipboard capture failed: {e:#}");
        }
    }
}

impl Handler {
    fn capture(&self) -> Result<()> {
        // Priority: image > files > html > rtf > text.
        //
        // macOS puts both image data AND file paths on the pasteboard when
        // you copy an image file (PNG/JPG/HEIC) from Finder or use
        // "Share → Copy Image" in many apps. Capturing as Files first meant
        // the user only ever saw the file path in history. Preferring image
        // here matches the "I copied a picture, store the picture"
        // expectation. Pure file copies (no image data) still fall through
        // to the Files branch below.
        if self.ctx.has(ContentFormat::Image) {
            if let Ok(img) = self.ctx.get_image() {
                let (w, h) = img.get_size();
                if let Ok(png) = img.to_png() {
                    let bytes = png.get_bytes();
                    if bytes.len() <= MAX_IMAGE_BYTES {
                        let b64 = B64.encode(bytes);
                        let text = format!("[image {}×{} · {} B]", w, h, bytes.len());
                        let byte_size = bytes.len() as i64;
                        self.store(NewClip {
                            content_type: ContentType::Image,
                            content_text: text,
                            content_data: b64,
                            byte_size,
                        })?;
                        return Ok(());
                    } else {
                        tracing::debug!(
                            "image skipped: {} bytes exceeds cap {}",
                            bytes.len(),
                            MAX_IMAGE_BYTES
                        );
                    }
                }
            }
        }
        if self.ctx.has(ContentFormat::Files) {
            if let Ok(paths) = self.ctx.get_files() {
                if !paths.is_empty() {
                    let json = serde_json::to_string(&paths)?;
                    let text = paths.join("\n");
                    let byte_size = json.len() as i64;
                    self.store(NewClip {
                        content_type: ContentType::Files,
                        content_text: text,
                        content_data: json,
                        byte_size,
                    })?;
                    return Ok(());
                }
            }
        }
        if self.ctx.has(ContentFormat::Html) {
            if let Ok(html) = self.ctx.get_html() {
                if !html.trim().is_empty() {
                    let text = strip_html(&html);
                    let byte_size = html.len() as i64;
                    self.store(NewClip {
                        content_type: ContentType::Html,
                        content_text: text,
                        content_data: html,
                        byte_size,
                    })?;
                    return Ok(());
                }
            }
        }
        if self.ctx.has(ContentFormat::Rtf) {
            if let Ok(rtf) = self.ctx.get_rich_text() {
                if !rtf.trim().is_empty() {
                    let text = strip_rtf(&rtf);
                    let byte_size = rtf.len() as i64;
                    self.store(NewClip {
                        content_type: ContentType::Rtf,
                        content_text: text,
                        content_data: rtf,
                        byte_size,
                    })?;
                    return Ok(());
                }
            }
        }
        if self.ctx.has(ContentFormat::Text) {
            if let Ok(text) = self.ctx.get_text() {
                if !text.trim().is_empty() {
                    let byte_size = text.len() as i64;
                    self.store(NewClip {
                        content_type: ContentType::Text,
                        content_text: text.clone(),
                        content_data: text,
                        byte_size,
                    })?;
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    fn store(&self, clip: NewClip) -> Result<()> {
        // If this event matches a payload *we* just wrote (paste action),
        // consume the fuse and skip — no duplicate history entry.
        let payload_hash = hash_payload(clip.content_type, &clip.content_data);
        {
            let mut self_written = self.self_written.lock();
            if self_written.as_deref() == Some(payload_hash.as_str()) {
                *self_written = None;
                return Ok(());
            }
        }

        // App exclusion: when the frontmost app (the one that just copied) is
        // on the user's exclude list — password managers etc. — drop the clip
        // silently so secrets never reach the history. Only pay the frontmost
        // lookup when the list is non-empty.
        let exclude =
            crate::settings::get_or(&self.db, KEY_EXCLUDE_APPS, "").unwrap_or_default();
        if !exclude.trim().is_empty() {
            if let Some(front) = crate::frontmost_app::name() {
                if is_excluded_app(&front, &exclude) {
                    return Ok(());
                }
            }
        }

        let _id = upsert_clip(&self.db, &clip)?;
        let _ = self.app.emit("clipboard-changed", ());

        // Auto-clear: wipe the system clipboard N seconds after this copy,
        // unless a newer copy supersedes it first (generation guard). Opt-in
        // (0 = off).
        let secs = crate::settings::get_or(&self.db, KEY_AUTO_CLEAR_SECS, "0")
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let my_gen = self.clear_gen.fetch_add(1, Ordering::SeqCst) + 1;
        if secs > 0 {
            let clear_gen = self.clear_gen.clone();
            let app = self.app.clone();
            let self_written = self.self_written.clone();
            thread::spawn(move || {
                // Sleep in short chunks instead of one long sleep so a superseded
                // timer exits within ~1 s of the next copy, rather than lingering
                // for the full (up to 3600 s) window. This bounds the number of
                // concurrently-sleeping auto-clear threads under rapid copying.
                let mut remaining = secs;
                while remaining > 0 {
                    if clear_gen.load(Ordering::SeqCst) != my_gen {
                        return; // a newer clip owns the clipboard now
                    }
                    let step = remaining.min(1);
                    thread::sleep(Duration::from_secs(step));
                    remaining -= step;
                }
                // A newer clip arrived → its own timer owns the clipboard now.
                if clear_gen.load(Ordering::SeqCst) != my_gen {
                    return;
                }
                if let Ok(ctx) = ClipboardContext::new() {
                    // Arm the self-write fuse so clearing doesn't capture an
                    // empty entry, then blank the clipboard.
                    *self_written.lock() = Some(hash_payload(ContentType::Text, ""));
                    let _ = ctx.set_text(String::new());
                    let _ = app.emit("clipboard-changed", ());
                }
            });
        }
        Ok(())
    }
}

pub fn spawn(
    app: AppHandle,
    db: DbHandle,
    paused: Arc<AtomicBool>,
    self_written: Arc<Mutex<Option<String>>>,
) {
    thread::Builder::new()
        .name("clipboard-watcher".into())
        .spawn(move || {
            let ctx = match ClipboardContext::new() {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("clipboard context init failed: {e:?}");
                    return;
                }
            };
            let mut watcher = match ClipboardWatcherContext::new() {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!("clipboard watcher init failed: {e:?}");
                    return;
                }
            };
            watcher.add_handler(Handler {
                ctx,
                db,
                app,
                paused,
                self_written,
                clear_gen: Arc::new(AtomicU64::new(0)),
            });
            watcher.start_watch();
        })
        .expect("failed to spawn clipboard watcher thread");
}

/// Extremely minimal RTF → plain-text extractor: strips control words and
/// braces so the preview is readable. RTF paste itself uses the raw payload.
fn strip_rtf(rtf: &str) -> String {
    let mut out = String::with_capacity(rtf.len() / 2);
    let mut in_ctrl = false;
    for ch in rtf.chars() {
        match ch {
            '\\' => {
                in_ctrl = true;
            }
            '{' | '}' => {
                in_ctrl = false;
            }
            ' ' | '\n' | '\r' | '\t' if in_ctrl => {
                in_ctrl = false;
            }
            _ if in_ctrl => {}
            _ => out.push(ch),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extremely minimal HTML → plain-text: drops tags.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{is_excluded_app, strip_html, strip_rtf, WatcherState};

    #[test]
    fn excluded_app_matches_case_insensitive_substring() {
        let list = "1Password, KeePassXC\nBitwarden";
        assert!(is_excluded_app("1Password 8", list));
        assert!(is_excluded_app("keepassxc", list)); // case-insensitive
        assert!(is_excluded_app("Bitwarden", list));
        assert!(!is_excluded_app("Safari", list));
        assert!(!is_excluded_app("Notes", list));
    }

    #[test]
    fn excluded_app_empty_list_never_matches() {
        assert!(!is_excluded_app("1Password", ""));
        assert!(!is_excluded_app("anything", "   \n  ,  "));
    }

    #[test]
    fn excluded_app_ignores_blank_entries_and_trims() {
        let list = "  , Slack ,\n\n  Discord  ";
        assert!(is_excluded_app("Slack", list));
        assert!(is_excluded_app("discord canary", list));
        assert!(!is_excluded_app("x", list)); // a lone blank entry must not match all
    }
    use crate::db::hash_payload;
    use crate::models::ContentType;

    #[test]
    fn strip_html_removes_simple_tags() {
        assert_eq!(strip_html("<p>Hello <b>world</b></p>"), "Hello world");
    }

    #[test]
    fn strip_html_self_closing_tag() {
        // Tags are dropped without inserting a space; adjacent text merges.
        assert_eq!(strip_html("line1<br/>line2"), "line1line2");
    }

    #[test]
    fn strip_html_collapses_whitespace() {
        assert_eq!(strip_html("a  <span>  </span>  b"), "a b");
    }

    #[test]
    fn strip_html_plain_text_passes_through() {
        assert_eq!(strip_html("no tags here"), "no tags here");
    }

    #[test]
    fn strip_html_empty_input() {
        assert_eq!(strip_html(""), "");
    }

    #[test]
    fn strip_rtf_removes_control_words() {
        let rtf = r"{\rtf1\ansi Hello {\b world}}";
        let result = strip_rtf(rtf);
        assert!(result.contains("Hello"), "expected 'Hello' in {result:?}");
        assert!(result.contains("world"), "expected 'world' in {result:?}");
    }

    #[test]
    fn strip_rtf_plain_text_passes_through() {
        assert_eq!(strip_rtf("Hello world"), "Hello world");
    }

    #[test]
    fn strip_rtf_empty_input() {
        assert_eq!(strip_rtf(""), "");
    }

    #[test]
    fn strip_rtf_collapses_whitespace() {
        let result = strip_rtf("a   b");
        assert_eq!(result, "a b");
    }

    #[test]
    fn strip_rtf_handles_realistic_rtf_doc() {
        // Excerpt that resembles what TextEdit / Word actually outputs.
        let rtf = r#"{\rtf1\ansi\ansicpg1252\cocoartf2761
\fonttbl\f0\fswiss\fcharset0 Helvetica;
{\colortbl;\red255\green255\blue255;}
\paperw11900\paperh16840\margl1440\margr1440\vieww11520\viewh8400\viewkind0
\pard\tx566\tx1133\tx1700\tx2267\tx2834\tx3401\tx3968\tx4535\tx5102\tx5669\pardirnatural
\f0\fs24 \cf0 Hello world this is a test.}"#;
        let plain = strip_rtf(rtf);
        assert!(plain.contains("Hello world this is a test"),
            "stripped text should preserve readable content: {plain:?}");
        // Control words must be gone.
        assert!(!plain.contains("\\rtf"));
        assert!(!plain.contains("\\fonttbl"));
    }

    #[test]
    fn strip_rtf_handles_escaped_braces_and_backslashes() {
        // RTF escapes literal { } \ with leading backslashes.
        let rtf = r"{\rtf1 plain text with \{ literal \} braces and \\ slash}";
        let plain = strip_rtf(rtf);
        assert!(plain.contains("plain text"));
    }

    #[test]
    fn strip_rtf_returns_empty_for_only_control_words() {
        let rtf = r"{\rtf1\ansi\ansicpg1252}";
        let plain = strip_rtf(rtf);
        assert!(plain.trim().is_empty(),
            "RTF doc with no actual text should reduce to whitespace: {plain:?}");
    }

    #[test]
    fn mark_self_write_arms_the_next_event_to_be_skipped() {
        // mark_self_write stores hash_payload(type, data). The next watcher
        // event hashes its observation and skips if it matches.
        let state = WatcherState::new();
        state.mark_self_write(ContentType::Text, "hello");
        let armed = state.self_written.lock().clone();
        assert_eq!(armed.as_ref(), Some(&hash_payload(ContentType::Text, "hello")));
    }

    #[test]
    fn mark_self_write_overwrites_prior_arming() {
        // Each call replaces the prior fuse; only the *last* armed type+payload
        // is active when the next watcher event lands.
        let state = WatcherState::new();
        state.mark_self_write(ContentType::Text, "hello");
        state.mark_self_write(ContentType::Image, "base64data==");
        let armed = state.self_written.lock().clone();
        assert_eq!(armed.as_ref(), Some(&hash_payload(ContentType::Image, "base64data==")));
        assert_ne!(armed.as_ref(), Some(&hash_payload(ContentType::Text, "hello")));
    }

    #[test]
    fn mark_self_write_different_content_types_distinguish_via_hash() {
        // Same payload string under different ContentType must produce
        // distinct fuses — protects against an Image whose decoded base64
        // ever colliding with a Text payload.
        let state1 = WatcherState::new();
        state1.mark_self_write(ContentType::Text, "shared");
        let h1 = state1.self_written.lock().clone();
        let state2 = WatcherState::new();
        state2.mark_self_write(ContentType::Html, "shared");
        let h2 = state2.self_written.lock().clone();
        assert_ne!(h1, h2);
    }
}
