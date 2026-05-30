//! Standalone Markdown → PDF conversion. **v0.46.0:** no longer
//! depends on the external `mrxdown` CLI — the full pipeline runs
//! in-process via `pulldown-cmark` (MD → HTML, CommonMark + GFM
//! extensions) + platform-native HTML → PDF rendering (WKWebView on
//! macOS).
//!
//! ## Pipeline
//!
//!   .md / .markdown
//!      │  pulldown_cmark::Parser (CommonMark + GFM tables, footnotes,
//!      │  strikethrough, task-lists)
//!      ▼
//!   <html>{embedded GitHub-flavored CSS}{rendered body}</html>
//!      │  WKWebView.createPDF (macOS 11+)  / TODO Win+Linux
//!      ▼
//!   .pdf next to source
//!
//! Output PDF lands sibling to input — `foo.md` → `foo.pdf` in the
//! same directory. Same convention as the old mrxdown shell-out so
//! the user-facing behaviour is unchanged.
//!
//! Triggered from Finder selection via `Ctrl+Shift+M` (handler in
//! `hotkey::register`). For each selected `.md` file we dispatch the
//! WKWebView render to the **main thread** (AppKit / WebKit are
//! main-thread-only; calling from the hotkey worker would crash).
//! That's a small UI-pause per file (~50-150 ms typically) — acceptable
//! for a one-shot batch action.

use std::path::{Path, PathBuf};

const MD_EXTENSIONS: &[&str] = &["md", "markdown"];

/// Result of a batch conversion call. `skipped` covers Finder
/// selections that aren't markdown (PNG, folder, …) — we don't treat
/// them as errors, just filter them out + report the count.
#[derive(Debug, Default)]
pub struct ConvertSummary {
    pub converted: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
    /// True on platforms where the native HTML→PDF backend isn't
    /// implemented yet (currently Windows + Linux). Drives a distinct
    /// "not yet supported here" notification instead of N×spawn-failed.
    pub backend_unavailable: bool,
}

/// **Synchronous, runs on the caller's thread.** The macOS WKWebView
/// rendering MUST run on the main thread; the caller (hotkey worker)
/// is responsible for dispatching there via `app.run_on_main_thread`.
///
/// Non-md paths land in `skipped`. Per-file failures land in
/// `failed`. Never panics; one bad file doesn't stop the rest.
pub fn convert_files(paths: &[PathBuf]) -> ConvertSummary {
    let mut summary = ConvertSummary::default();
    let mut md_files = Vec::new();
    for p in paths {
        if is_markdown(p) {
            md_files.push(p.clone());
        } else {
            summary.skipped.push(p.clone());
        }
    }
    if md_files.is_empty() {
        return summary;
    }
    if !backend_available() {
        summary.backend_unavailable = true;
        for p in md_files {
            summary
                .failed
                .push((p, "PDF-Backend auf dieser Platform noch nicht implementiert".into()));
        }
        return summary;
    }
    for p in md_files {
        match convert_single(&p) {
            Ok(()) => summary.converted.push(p),
            Err(e) => summary.failed.push((p, e)),
        }
    }
    summary
}

fn is_markdown(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|ext| MD_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn convert_single(input: &Path) -> Result<(), String> {
    let md_text =
        std::fs::read_to_string(input).map_err(|e| format!("can't read {input:?}: {e}"))?;
    let html = render_html(&md_text);
    let output = sibling_pdf_path(input);
    write_pdf(&html, &output)?;
    Ok(())
}

/// Replace the input's extension with `.pdf`. The is_markdown gate
/// upstream guarantees there IS an extension, but the fallback keeps
/// the function total.
fn sibling_pdf_path(input: &Path) -> PathBuf {
    let mut out = input.to_path_buf();
    let new_name = match input.file_stem().and_then(|s| s.to_str()) {
        Some(stem) => format!("{stem}.pdf"),
        None => "output.pdf".to_string(),
    };
    out.set_file_name(new_name);
    out
}

// ── MD → HTML ──────────────────────────────────────────────────────

/// Render markdown into a complete self-contained HTML document with
/// embedded CSS. The CSS is a GitHub-flavored-Markdown-inspired
/// stylesheet — sober typography, syntax-friendly code blocks, table
/// borders. No external resources (no web fonts, no CDN CSS) so the
/// renderer never needs network access.
///
/// **Public** for testability: the HTML output is fully deterministic
/// and easy to assert on without firing up a WKWebView.
pub fn render_html(md: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);

    let parser = Parser::new_ext(md, options);
    let mut body = String::new();
    html::push_html(&mut body, parser);

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<title>Markdown</title>
<style>
:root {{
  color-scheme: light;
  --fg: #1f2328;
  --muted: #59636e;
  --accent: #0969da;
  --border: #d1d9e0;
  --code-bg: #f6f8fa;
  --code-fg: #1f2328;
  --table-stripe: #f6f8fa;
}}
html {{ font-size: 16px; }}
body {{
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
  color: var(--fg);
  background: #fff;
  margin: 0 auto;
  padding: 32px 40px;
  line-height: 1.6;
  max-width: 800px;
}}
h1, h2, h3, h4, h5, h6 {{ line-height: 1.25; margin-top: 1.8em; margin-bottom: 0.6em; font-weight: 600; }}
h1 {{ font-size: 2em;    border-bottom: 1px solid var(--border); padding-bottom: 0.3em; }}
h2 {{ font-size: 1.5em;  border-bottom: 1px solid var(--border); padding-bottom: 0.3em; }}
h3 {{ font-size: 1.25em; }}
h4 {{ font-size: 1em; }}
h5 {{ font-size: 0.875em; color: var(--muted); }}
h6 {{ font-size: 0.85em;  color: var(--muted); }}
p, ul, ol, blockquote, pre, table {{ margin-top: 0; margin-bottom: 1em; }}
a {{ color: var(--accent); text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
ul, ol {{ padding-left: 2em; }}
li + li {{ margin-top: 0.25em; }}
li > input[type="checkbox"] {{ margin-right: 0.4em; }}
blockquote {{
  margin-left: 0;
  padding: 0 1em;
  color: var(--muted);
  border-left: 0.25em solid var(--border);
}}
hr {{
  border: 0;
  border-top: 1px solid var(--border);
  margin: 2em 0;
}}
code, pre, kbd {{
  font-family: "SF Mono", SFMono-Regular, ui-monospace, Menlo, Consolas, monospace;
  font-size: 0.92em;
}}
code {{
  background: var(--code-bg);
  color: var(--code-fg);
  padding: 0.2em 0.4em;
  border-radius: 4px;
}}
pre {{
  background: var(--code-bg);
  padding: 16px;
  border-radius: 6px;
  overflow-x: auto;
  line-height: 1.45;
}}
pre code {{
  background: transparent;
  padding: 0;
  border-radius: 0;
}}
table {{
  border-collapse: collapse;
  display: block;
  overflow-x: auto;
  width: 100%;
}}
th, td {{
  border: 1px solid var(--border);
  padding: 8px 12px;
  text-align: left;
}}
thead th {{ background: var(--code-bg); font-weight: 600; }}
tr:nth-child(2n) td {{ background: var(--table-stripe); }}
img {{ max-width: 100%; height: auto; }}
@media print {{
  body {{
    padding: 24px 32px;
    max-width: none;
    -webkit-print-color-adjust: exact;
    print-color-adjust: exact;
  }}
  pre, blockquote, table {{ page-break-inside: avoid; }}
  h1, h2, h3, h4, h5, h6 {{ page-break-after: avoid; }}
}}
</style>
</head>
<body>
{body}
</body>
</html>"##,
        body = body
    )
}

// ── HTML → PDF (platform-native) ──────────────────────────────────

#[cfg(target_os = "macos")]
fn backend_available() -> bool {
    // WKWebView.createPDF needs macOS 11 (Big Sur, 2020). We don't
    // probe at runtime — the minimum supported macOS in
    // `macos/src-tauri/tauri.conf.json` is 10.15 today, but in
    // practice every user has 11+. If we ever ship to a 10.15 box
    // the createPDF call would just no-op + leave an empty file.
    true
}

#[cfg(not(target_os = "macos"))]
fn backend_available() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn write_pdf(html: &str, output: &Path) -> Result<(), String> {
    macos::render_html_to_pdf(html, output)
}

#[cfg(not(target_os = "macos"))]
fn write_pdf(_html: &str, _output: &Path) -> Result<(), String> {
    Err("PDF-Rendering noch nicht implementiert auf dieser Platform".into())
}

#[cfg(target_os = "macos")]
mod macos {
    //! WKWebView-based HTML → PDF on macOS. Three stages:
    //!
    //! 1. Create an offscreen WKWebView (no window required).
    //! 2. `loadHTMLString` + spin the run loop until `isLoading` flips
    //!    false (self-contained HTML loads in <100 ms typically).
    //! 3. `createPDFWithConfiguration:completionHandler:` — also async,
    //!    so we block on a channel that the completion block fills.
    //!
    //! **Main-thread only.** The caller (hotkey worker) must
    //! dispatch via `app.run_on_main_thread` before invoking. AppKit
    //! / WebKit assert this internally.

    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2::{Encode, Encoding, RefEncode};
    use std::ffi::{c_void, CStr};
    use std::path::Path;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    type Id = *mut AnyObject;
    const NIL: Id = std::ptr::null_mut();

    /// Per-render result. The completion handler stores either the
    /// PDF bytes or a stringified error here, wakes the channel.
    enum PdfResult {
        Ok(Vec<u8>),
        Err(String),
    }

    pub fn render_html_to_pdf(html: &str, output: &Path) -> Result<(), String> {
        unsafe {
            // ── 1) Build WKWebView ────────────────────────────────────
            // Frame mimics US-Letter at 96 DPI ≈ 800×1100 — close enough
            // to print intent. createPDF uses the view's bounds when
            // no per-page rect is set in the configuration.
            let frame = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize { width: 800.0, height: 1100.0 },
            };
            let config_class = objc2::class!(WKWebViewConfiguration);
            let config: Id = msg_send![config_class, new];
            let webview_class = objc2::class!(WKWebView);
            let webview: Id = msg_send![webview_class, alloc];
            let webview: Id =
                msg_send![webview, initWithFrame: frame, configuration: config];
            // We own `config` via `new` (returns +1), release it now —
            // the WKWebView keeps its own retain.
            release(config);
            if webview.is_null() {
                return Err("WKWebView alloc/init returned nil".into());
            }
            // `alloc + init` returns a +1 reference we already own;
            // no need to retain again. We'll release at the end.

            // ── 2) Load HTML, spin run loop until done ─────────────────
            let html_nsstring = nsstring_from_str(html);
            // baseURL: nil → relative links would fail, but we don't
            // expect any in pasted markdown.
            let _: Id = msg_send![webview,
                loadHTMLString: html_nsstring,
                baseURL: NIL];
            release(html_nsstring);

            // Spin the run loop until isLoading == NO. Cap at 5 s as
            // a sanity stop (self-contained HTML with no external
            // resources should never need more than a few hundred ms).
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let is_loading: bool = msg_send![webview, isLoading];
                if !is_loading {
                    break;
                }
                if Instant::now() > deadline {
                    release(webview);
                    return Err("WKWebView load timed out after 5s".into());
                }
                run_loop_pump(Duration::from_millis(30));
            }
            // Even after isLoading drops, the layout/render pass may
            // not be flushed. A tiny extra pump catches edge cases
            // where the next createPDF would otherwise capture an
            // empty page.
            run_loop_pump(Duration::from_millis(50));

            // ── 3) createPDF with completion block ────────────────────
            let result: Arc<Mutex<Option<PdfResult>>> = Arc::new(Mutex::new(None));
            let (tx, rx) = mpsc::channel::<()>();

            let result_for_block = Arc::clone(&result);
            let tx_for_block = Mutex::new(Some(tx));
            // block2::RcBlock builds a heap-allocated Objective-C
            // block. The completion handler runs on the main thread
            // (where we are now), captures the result + sends the
            // wake-up signal.
            let block = block2::RcBlock::new(move |data: Id, error: Id| {
                let outcome = if !error.is_null() {
                    let desc: Id = msg_send![error, localizedDescription];
                    PdfResult::Err(nsstring_to_string(desc).unwrap_or_else(|| {
                        "createPDF returned error (no description)".to_string()
                    }))
                } else if data.is_null() {
                    PdfResult::Err("createPDF returned nil data".to_string())
                } else {
                    let bytes_ptr: *const u8 = msg_send![data, bytes];
                    let len: usize = msg_send![data, length];
                    if bytes_ptr.is_null() || len == 0 {
                        PdfResult::Err("createPDF data was empty".to_string())
                    } else {
                        let slice = std::slice::from_raw_parts(bytes_ptr, len);
                        PdfResult::Ok(slice.to_vec())
                    }
                };
                if let Ok(mut guard) = result_for_block.lock() {
                    *guard = Some(outcome);
                }
                if let Ok(mut sender) = tx_for_block.lock() {
                    if let Some(s) = sender.take() {
                        let _ = s.send(());
                    }
                }
            });

            // createPDFWithConfiguration: nil uses the view's current
            // bounds for the page rect. That's what we want — our
            // CSS @media print rules tighten margins for the PDF.
            let _: () = msg_send![webview,
                createPDFWithConfiguration: NIL,
                completionHandler: &*block];

            // Pump the run loop until the completion fires (signalled
            // via the channel) or we hit a 10 s ceiling.
            let pdf_deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if rx.try_recv().is_ok() {
                    break;
                }
                if Instant::now() > pdf_deadline {
                    release(webview);
                    return Err("createPDF timed out after 10s".into());
                }
                run_loop_pump(Duration::from_millis(30));
            }

            let pdf_bytes = {
                let mut guard = result
                    .lock()
                    .map_err(|e| format!("result mutex poisoned: {e}"))?;
                match guard.take() {
                    Some(PdfResult::Ok(bytes)) => bytes,
                    Some(PdfResult::Err(e)) => {
                        release(webview);
                        return Err(format!("createPDF: {e}"));
                    }
                    None => {
                        release(webview);
                        return Err("createPDF channel fired without storing result".into());
                    }
                }
            };

            release(webview);

            std::fs::write(output, &pdf_bytes)
                .map_err(|e| format!("can't write PDF to {output:?}: {e}"))?;
        }
        Ok(())
    }

    /// Iterate the main CFRunLoop for up to `dur`. Returns when one
    /// event is processed OR the timeout elapses — we call this in
    /// a poll loop, not a single big wait.
    fn run_loop_pump(dur: Duration) {
        unsafe {
            CFRunLoopRunInMode(
                k_cf_run_loop_default_mode(),
                dur.as_secs_f64(),
                true, // returnAfterSourceHandled — break on first event
            );
        }
    }

    // ── Tiny NSString / NSData helpers ────────────────────────────────

    unsafe fn nsstring_from_str(s: &str) -> Id {
        let bytes = s.as_bytes();
        let nsstring_class = objc2::class!(NSString);
        let inst: Id = msg_send![nsstring_class, alloc];
        // NSUTF8StringEncoding = 4
        let inst: Id = msg_send![inst,
            initWithBytes: bytes.as_ptr() as *const c_void,
            length: bytes.len(),
            encoding: 4_usize];
        inst
    }

    unsafe fn nsstring_to_string(nsstring: Id) -> Option<String> {
        if nsstring.is_null() {
            return None;
        }
        let utf8: *const std::os::raw::c_char = msg_send![nsstring, UTF8String];
        if utf8.is_null() {
            return None;
        }
        CStr::from_ptr(utf8).to_str().ok().map(|s| s.to_string())
    }

    unsafe fn release(obj: Id) {
        if !obj.is_null() {
            let _: () = msg_send![obj, release];
        }
    }

    // ── CG / CF types we use directly via FFI ─────────────────────────

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct CGPoint { x: f64, y: f64 }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct CGSize { width: f64, height: f64 }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct CGRect { origin: CGPoint, size: CGSize }

    // Objective-C type encodings — required so `msg_send!` can build
    // the correct method signature when CGRect / CGSize / CGPoint
    // are passed as struct-by-value arguments.
    unsafe impl Encode for CGPoint {
        const ENCODING: Encoding =
            Encoding::Struct("CGPoint", &[<f64 as Encode>::ENCODING, <f64 as Encode>::ENCODING]);
    }
    unsafe impl RefEncode for CGPoint {
        const ENCODING_REF: Encoding = Encoding::Pointer(&<CGPoint as Encode>::ENCODING);
    }
    unsafe impl Encode for CGSize {
        const ENCODING: Encoding =
            Encoding::Struct("CGSize", &[<f64 as Encode>::ENCODING, <f64 as Encode>::ENCODING]);
    }
    unsafe impl RefEncode for CGSize {
        const ENCODING_REF: Encoding = Encoding::Pointer(&<CGSize as Encode>::ENCODING);
    }
    unsafe impl Encode for CGRect {
        const ENCODING: Encoding = Encoding::Struct(
            "CGRect",
            &[<CGPoint as Encode>::ENCODING, <CGSize as Encode>::ENCODING],
        );
    }
    unsafe impl RefEncode for CGRect {
        const ENCODING_REF: Encoding = Encoding::Pointer(&<CGRect as Encode>::ENCODING);
    }

    type CFStringRef = *const c_void;
    type CFRunLoopMode = CFStringRef;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRunLoopRunInMode(mode: CFRunLoopMode, seconds: f64, return_after_source_handled: bool) -> i32;
        static kCFRunLoopDefaultMode: CFStringRef;
    }

    fn k_cf_run_loop_default_mode() -> CFRunLoopMode {
        // Accessed via &static — the variable itself is a non-owning
        // CFString constant living in the CoreFoundation framework.
        unsafe { kCFRunLoopDefaultMode }
    }
}

// ── Notification helpers ───────────────────────────────────────────

pub fn notify(summary: &ConvertSummary) {
    let msg = build_notification_message(summary);
    notify_visual(&msg);
    if !summary.failed.is_empty() {
        notify_audio_failure();
    } else if !summary.converted.is_empty() {
        notify_audio_success();
    }
}

/// One-line user-facing summary. German to match timer.rs.
/// Public for unit tests.
pub fn build_notification_message(summary: &ConvertSummary) -> String {
    let total = summary.converted.len() + summary.skipped.len() + summary.failed.len();
    if total == 0 {
        return "Keine Dateien selektiert".to_string();
    }
    if summary.backend_unavailable {
        return "Markdown → PDF: macOS-only in v0.46.0 (Win + Linux folgen)".to_string();
    }
    if summary.converted.is_empty() && summary.failed.is_empty() {
        return format!(
            "Keine Markdown-Dateien in der Selektion ({n} übersprungen)",
            n = summary.skipped.len()
        );
    }
    let mut parts = Vec::new();
    if !summary.converted.is_empty() {
        parts.push(format!("{} PDF erstellt", summary.converted.len()));
    }
    if !summary.skipped.is_empty() {
        parts.push(format!("{} übersprungen", summary.skipped.len()));
    }
    if !summary.failed.is_empty() {
        parts.push(format!("{} fehlgeschlagen", summary.failed.len()));
    }
    parts.join(", ")
}

#[cfg(target_os = "macos")]
fn notify_visual(msg: &str) {
    let safe = msg.replace('"', "'").replace('\\', "/");
    let script = format!(
        r#"display notification "{safe}" with title "Inspector Rust" subtitle "Markdown → PDF""#
    );
    let _ = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(not(target_os = "macos"))]
fn notify_visual(_msg: &str) {}

#[cfg(target_os = "macos")]
fn notify_audio_success() {
    let _ = std::process::Command::new("/usr/bin/afplay")
        .arg("/System/Library/Sounds/Glass.aiff")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(target_os = "macos")]
fn notify_audio_failure() {
    let _ = std::process::Command::new("/usr/bin/afplay")
        .arg("/System/Library/Sounds/Funk.aiff")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(not(target_os = "macos"))]
fn notify_audio_success() {}

#[cfg(not(target_os = "macos"))]
fn notify_audio_failure() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_markdown_accepts_md_and_markdown_case_insensitive() {
        assert!(is_markdown(Path::new("foo.md")));
        assert!(is_markdown(Path::new("foo.MD")));
        assert!(is_markdown(Path::new("foo.markdown")));
        assert!(is_markdown(Path::new("foo.Markdown")));
        assert!(is_markdown(Path::new("/tmp/path with space.md")));
    }

    #[test]
    fn is_markdown_rejects_non_md() {
        assert!(!is_markdown(Path::new("foo.txt")));
        assert!(!is_markdown(Path::new("foo.pdf")));
        assert!(!is_markdown(Path::new("foo")));
        assert!(!is_markdown(Path::new("README")));
        assert!(!is_markdown(Path::new("foo.md.bak")));
    }

    #[test]
    fn sibling_pdf_path_replaces_extension() {
        assert_eq!(
            sibling_pdf_path(Path::new("/tmp/notes.md")),
            PathBuf::from("/tmp/notes.pdf")
        );
        assert_eq!(
            sibling_pdf_path(Path::new("/tmp/notes.markdown")),
            PathBuf::from("/tmp/notes.pdf")
        );
        // Multi-dot stems keep all but the last segment in the stem.
        assert_eq!(
            sibling_pdf_path(Path::new("/tmp/v1.0.md")),
            PathBuf::from("/tmp/v1.0.pdf")
        );
    }

    #[test]
    fn render_html_wraps_body_with_doctype_and_style() {
        let html = render_html("# Hello\n\nworld");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<style>"));
        assert!(html.contains("<h1>Hello</h1>"));
        assert!(html.contains("<p>world</p>"));
    }

    #[test]
    fn render_html_supports_gfm_tables() {
        let md = "| a | b |\n|---|---|\n| 1 | 2 |";
        let html = render_html(md);
        assert!(html.contains("<table>"));
        assert!(html.contains("<th>a</th>"));
        assert!(html.contains("<td>1</td>"));
    }

    #[test]
    fn render_html_supports_strikethrough_and_tasklists() {
        let html = render_html("~~gone~~\n\n- [x] done\n- [ ] todo");
        assert!(html.contains("<del>gone</del>"));
        // pulldown-cmark renders task list checkboxes as
        // <input ... type="checkbox" disabled="" /> with "checked" only
        // on the [x] item. Assert on the disabled-checkbox marker
        // (present on both items) instead of pinning the exact attr ordering.
        assert!(html.contains("type=\"checkbox\""));
        assert!(html.contains("disabled"));
        assert!(html.contains("checked"));
    }

    #[test]
    fn render_html_supports_code_blocks_with_language_class() {
        let html = render_html("```rust\nfn main() {}\n```");
        assert!(html.contains("<pre><code class=\"language-rust\""));
    }

    #[test]
    fn empty_summary_says_nothing_selected() {
        let s = ConvertSummary::default();
        assert_eq!(build_notification_message(&s), "Keine Dateien selektiert");
    }

    #[test]
    fn only_non_md_says_nothing_to_convert() {
        let s = ConvertSummary {
            skipped: vec!["a.png".into(), "b.txt".into()],
            ..Default::default()
        };
        assert_eq!(
            build_notification_message(&s),
            "Keine Markdown-Dateien in der Selektion (2 übersprungen)"
        );
    }

    #[test]
    fn success_says_count() {
        let s = ConvertSummary {
            converted: vec!["a.md".into(), "b.md".into()],
            ..Default::default()
        };
        assert_eq!(build_notification_message(&s), "2 PDF erstellt");
    }

    #[test]
    fn backend_unavailable_message_is_actionable() {
        let s = ConvertSummary {
            backend_unavailable: true,
            failed: vec![("a.md".into(), "backend missing".into())],
            ..Default::default()
        };
        let msg = build_notification_message(&s);
        assert!(msg.contains("macOS-only"));
        assert!(!msg.contains("fehlgeschlagen"));
    }
}
