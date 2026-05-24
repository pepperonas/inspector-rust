use anyhow::Result;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use clipboard_rs::common::RustImage;
use clipboard_rs::{
    Clipboard, ClipboardContext, ClipboardHandler, ClipboardWatcher, ClipboardWatcherContext,
    ContentFormat,
};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tauri::{AppHandle, Emitter};

use crate::db::{hash_payload, upsert_clip, DbHandle};
use crate::models::{ContentType, NewClip, MAX_IMAGE_BYTES};

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
        let _id = upsert_clip(&self.db, &clip)?;
        let _ = self.app.emit("clipboard-changed", ());
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
    use super::{strip_html, strip_rtf, WatcherState};
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
