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
    /// The PDFs written, index-aligned with `converted` (the name may carry a
    /// Finder-style ` 2` when `foo.pdf` already existed).
    pub outputs: Vec<PathBuf>,
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
pub fn convert_files(paths: &[PathBuf], dark: bool) -> ConvertSummary {
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
                .push((p, crate::i18n::t("mdpdf.backend_unimpl")));
        }
        return summary;
    }
    for p in md_files {
        match convert_single(&p, dark) {
            Ok(out) => {
                summary.converted.push(p);
                summary.outputs.push(out);
            }
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

fn convert_single(input: &Path, dark: bool) -> Result<PathBuf, String> {
    let md_text =
        std::fs::read_to_string(input).map_err(|e| format!("can't read {input:?}: {e}"))?;
    let doc = build_document(&md_text, input.parent(), dark);
    // Never overwrite: `foo.pdf` taken → `foo 2.pdf`, `foo 3.pdf` … (Finder's
    // "keep both" naming).
    let output = free_pdf_path(input, |p| p.exists());
    write_document_pdf(&doc, &output)?;
    Ok(output)
}

/// The PDF path for `input`: same folder, same name, `.pdf` — and when that
/// file already exists, the first free `name N.pdf` from N = 2 upwards, the
/// way the Finder names a kept duplicate. `exists` is injected so the rule is
/// testable without touching the disk. Pure.
pub fn free_pdf_path(input: &Path, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let dir = input.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("output");
    let first = dir.join(format!("{stem}.pdf"));
    if !exists(&first) {
        return first;
    }
    for n in 2u32..=9999 {
        let candidate = dir.join(format!("{stem} {n}.pdf"));
        if !exists(&candidate) {
            return candidate;
        }
    }
    // 9 998 duplicates of one name: fall back to a timestamp rather than loop.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    dir.join(format!("{stem} {ts}.pdf"))
}

/// Which files `md2pdf` converts. An explicit path wins (typing it is the more
/// specific intent); otherwise the Markdown files of the Finder selection.
/// A path that isn't Markdown or doesn't exist is an error with a reason — the
/// command never silently falls through to something else. Pure (the file
/// check is injected).
pub fn choose_inputs(
    arg: Option<PathBuf>,
    selection: Result<Vec<PathBuf>, String>,
    is_file: impl Fn(&Path) -> bool,
) -> Result<Vec<PathBuf>, String> {
    if let Some(p) = arg {
        if !is_markdown(&p) {
            return Err(crate::i18n::tf("mdpdf.not_markdown", &[("path", &p.display().to_string())]));
        }
        if !is_file(&p) {
            return Err(crate::i18n::tf("mdpdf.not_found", &[("path", &p.display().to_string())]));
        }
        return Ok(vec![p]);
    }
    let selected = selection.unwrap_or_default();
    let md: Vec<PathBuf> = selected.into_iter().filter(|p| is_markdown(p)).collect();
    if md.is_empty() {
        return Err(crate::i18n::t("mdpdf.no_selection"));
    }
    Ok(md)
}

// ── MD → HTML (mrxdown look) ──────────────────────────────────────

const MRX_CSS: &str = include_str!("../assets/md2pdf/mrxdown-default.css");
const KATEX_CSS: &str = include_str!("../assets/md2pdf/katex.inline.css");
const HLJS_JS: &str = include_str!("../assets/md2pdf/highlight.min.js");
const KATEX_JS: &str = include_str!("../assets/md2pdf/katex.min.js");
const MERMAID_JS: &str = include_str!("../assets/md2pdf/mermaid.min.js");

/// mrxdown's highlight palette (`src/main/export/pdf-html.js::getHighlightCss`).
const HLJS_CSS: &str = r#"
.hljs { color: #1a1a1a; }
.hljs-keyword, .hljs-selector-tag, .hljs-built_in { color: #a626a4; font-weight: 600; }
.hljs-string, .hljs-attr { color: #50a14f; }
.hljs-number, .hljs-literal { color: #986801; }
.hljs-comment { color: #999; font-style: italic; }
.hljs-function .hljs-title, .hljs-title.function_ { color: #4078f2; }
.hljs-class .hljs-title, .hljs-title.class_ { color: #c18401; }
.hljs-type, .hljs-params { color: #c18401; }
.hljs-meta, .hljs-tag { color: #e45649; }
.hljs-variable, .hljs-template-variable { color: #e45649; }
.hljs-regexp { color: #50a14f; }
.hljs-symbol, .hljs-bullet { color: #4078f2; }
"#;

/// Additions for what mrxdown renders elsewhere (math/mermaid placement) and
/// for the print path: WebKit applies the page margins itself, so the body's
/// screen padding is dropped in print.
const EXTRA_CSS: &str = r#"
.ir-math-display { display: block; text-align: center; margin: 1em 0; }
.mermaid { text-align: center; margin: 1.2em 0; break-inside: avoid; }
.mermaid svg { max-width: 100%; height: auto; }
.mrx-titlepage .mrx-affiliation { font-size: 1rem; color: #555; margin-top: 0.5rem; }
.mrx-titlepage .mrx-abstract { margin: 3rem auto 0; max-width: 32rem; text-align: justify; font-size: 0.95rem; }
.mrx-titlepage .mrx-abstract-label { font-weight: 600; text-align: center; margin-bottom: 0.5rem; }
@media print { body { max-width: none; } }
a, p, li, td, th { overflow-wrap: anywhere; }
pre { white-space: pre-wrap; overflow-wrap: anywhere; }
pre code { white-space: pre-wrap; }
.footnote-definition { display: flex; gap: 0.5em; font-size: 0.9em; margin: 0.6em 0; }
.footnote-definition p { margin: 0; text-align: left; }
.footnote-definition-label { flex-shrink: 0; }
.ir-outline { font-size: 1px; line-height: 1px; height: 1px; margin: 0; padding: 0; }
.ir-outline a { color: transparent !important; text-decoration: none !important; }
"#;

/// The page colour of the dark theme (GitHub's dark canvas), also painted
/// into the page margins by the PDF writer.
pub const DARK_BG: (u8, u8, u8) = (0x0d, 0x11, 0x17);

/// `md2pdf --dark`: GitHub's dark colours on top of the base sheet.
const DARK_CSS: &str = r##"
@page { background: #0d1117; }
html, body { background: #0d1117 !important; color: #e6edf3 !important; }
h1, h2, h3, h4, h5, p, li, ul, ol, strong, b, em, i, th, td { color: #e6edf3 !important; }
h1 { border-bottom-color: #3d444d; }
h6 { color: #9198a1 !important; }
a, a[href^="#"] { color: #4493f8 !important; }
code { background: #262c36 !important; color: #e6edf3 !important; border-color: #3d444d; }
pre { background: #151b23 !important; color: #e6edf3 !important; border-color: #3d444d; }
pre code { background: none !important; color: #e6edf3 !important; }
blockquote { border-left-color: #3d444d; color: #9198a1 !important; background: transparent; }
blockquote p { color: #9198a1 !important; }
th, td { border-color: #3d444d; }
th { background: #151b23 !important; }
tr:nth-child(even) { background: #151b23 !important; }
tr:nth-child(odd) { background: #0d1117 !important; }
hr { border-top-color: #3d444d; }
.callout { background: #151b23 !important; border-left-color: #4493f8; }
.callout-title { color: #4493f8; }
.callout-tip { border-left-color: #3fb950; } .callout-tip .callout-title { color: #3fb950; }
.callout-important { border-left-color: #ab7df8; } .callout-important .callout-title { color: #ab7df8; }
.callout-warning { border-left-color: #d29922; } .callout-warning .callout-title { color: #d29922; }
.callout-caution { border-left-color: #f85149; } .callout-caution .callout-title { color: #f85149; }
.mrx-titlepage .mrx-subtitle, .mrx-titlepage .mrx-affiliation, .mrx-titlepage .mrx-date, .mrx-titlepage .mrx-meta { color: #9198a1; }
.hljs { color: #e6edf3; }
.hljs-keyword, .hljs-selector-tag, .hljs-built_in { color: #ff7b72; }
.hljs-string, .hljs-attr, .hljs-regexp { color: #a5d6ff; }
.hljs-number, .hljs-literal, .hljs-symbol, .hljs-bullet { color: #79c0ff; }
.hljs-comment { color: #8b949e; }
.hljs-function .hljs-title, .hljs-title.function_ { color: #d2a8ff; }
.hljs-class .hljs-title, .hljs-title.class_, .hljs-type, .hljs-params, .hljs-variable, .hljs-template-variable { color: #ffa657; }
.hljs-meta, .hljs-tag { color: #7ee787; }
"##;

/// Lucide icons + German labels, 1:1 from mrxdown `callouts.js`.
fn callout_meta(kind: pulldown_cmark::BlockQuoteKind) -> (&'static str, &'static str, &'static str) {
    use pulldown_cmark::BlockQuoteKind as K;
    match kind {
        K::Note => ("note", "Hinweis", r#"<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>"#),
        K::Tip => ("tip", "Tipp", r#"<path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5"/><path d="M9 18h6"/><path d="M10 22h4"/>"#),
        K::Important => ("important", "Wichtig", r#"<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/><path d="M12 7v2"/><path d="M12 13h.01"/>"#),
        K::Warning => ("warning", "Warnung", r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 20h16a2 2 0 0 0 1.73-2Z"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#),
        K::Caution => ("caution", "Achtung", r#"<path d="M7.86 2h8.28L22 7.86v8.28L16.14 22H7.86L2 16.14V7.86L7.86 2z"/><path d="M12 8v4"/><path d="M12 16h.01"/>"#),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Frontmatter fields for the title page (`title`, `subtitle`, `author`,
/// `affiliation`, `date`, `abstract`). Deliberately a tiny `key: value` reader
/// — the title page needs flat strings, not a YAML engine. Pure.
pub fn parse_frontmatter(yaml: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for line in yaml.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let k = k.trim().to_ascii_lowercase();
        if k.is_empty() || k.starts_with('#') || line.starts_with(' ') {
            continue;
        }
        let v = v.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        if !v.is_empty() {
            out.insert(k, v.to_string());
        }
    }
    out
}

/// mrxdown's title page (`pdf-templates.js::renderTitlePage`): only when the
/// frontmatter has a title.
fn title_page(fm: &std::collections::HashMap<String, String>) -> String {
    let Some(title) = fm.get("title") else { return String::new() };
    let mut parts = vec![format!("<h1>{}</h1>", esc(title))];
    for (key, class) in [
        ("subtitle", "mrx-subtitle"),
        ("author", "mrx-author"),
        ("affiliation", "mrx-affiliation"),
        ("date", "mrx-date"),
    ] {
        if let Some(v) = fm.get(key) {
            parts.push(format!("<div class=\"{class}\">{}</div>", esc(v)));
        }
    }
    if let Some(a) = fm.get("abstract") {
        parts.push(format!(
            "<div class=\"mrx-abstract\"><div class=\"mrx-abstract-label\">{}</div>{}</div>",
            crate::i18n::t("mdpdf.abstract"),
            esc(a)
        ));
    }
    format!("<div class=\"mrx-titlepage\">{}</div>", parts.join(""))
}

/// Percent-decode a URL path (`%20` → space). Invalid sequences stay literal.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn image_mime(p: &Path) -> Option<&'static str> {
    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        _ => return None,
    })
}

/// A local image (relative to the document, absolute, `file://` or `~/…`)
/// as a data URI — the page is loaded from a string, so a file path could
/// never resolve (mrxdown embeds the same way). Remote URLs and unknown types
/// stay untouched; the CSP then keeps remote ones from loading.
fn embed_image(dest: &str, base: Option<&Path>) -> Option<String> {
    use base64::Engine;
    let raw = dest.trim();
    if raw.is_empty() || raw.starts_with("data:") {
        return None;
    }
    let path_str = if let Some(rest) = raw.strip_prefix("file://") {
        rest
    } else if raw.contains("://") {
        return None;
    } else {
        raw
    };
    let decoded = percent_decode(path_str);
    let path = crate::path_arg::expand_user(&decoded, dirs::home_dir().as_deref());
    let path = if path.is_absolute() {
        path
    } else {
        base?.join(path)
    };
    let mime = image_mime(&path)?;
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > 25 * 1024 * 1024 {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// Render markdown into a complete, self-contained HTML document in mrxdown's
/// default PDF look: its stylesheet, callouts, title page from frontmatter,
/// embedded local images, and — only when the document needs them —
/// highlight.js, KaTeX and Mermaid, executed in the page before printing.
///
/// Same safety model as mrxdown's CLI path: a CSP that allows ONLY the
/// nonce-carrying scripts we inject, so a `<script>` inside a foreign `.md`
/// never runs, and no network access (images are embedded, `img-src data:`).
///
/// The page sets `window.__irDone = true` once highlighting, math, diagrams,
/// fonts and images are ready; the renderer waits for it.
/// A heading of the document: its level, plain text and anchor id.
#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub id: String,
}

/// What `build_document` hands the PDF writer.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub html: String,
    /// Frontmatter `title`, else the first heading.
    pub title: Option<String>,
    pub author: Option<String>,
    /// Every heading in order — the PDF's outline (bookmarks).
    pub headings: Vec<Heading>,
    pub dark: bool,
}

/// GitHub's anchor for a heading: lower case, letters/digits/`-`/`_` kept,
/// spaces become `-`, everything else is dropped ("Code & Mathe" →
/// `code--mathe`). Pure.
pub fn slugify(text: &str) -> String {
    let mut out = String::new();
    for c in text.trim().chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.extend(c.to_lowercase());
        } else if c.is_whitespace() {
            out.push('-');
        }
    }
    if out.is_empty() {
        out.push_str("section");
    }
    out
}

/// A slug not used before in this document: the second "Setup" becomes
/// `setup-1`, like on GitHub. Pure.
pub fn unique_slug(seen: &mut std::collections::HashSet<String>, slug: String) -> String {
    if seen.insert(slug.clone()) {
        return slug;
    }
    let mut n = 1;
    loop {
        let candidate = format!("{slug}-{n}");
        if seen.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

/// Where a bare web address starts at byte `i` of `s`, if it does.
fn url_start(s: &str, i: usize) -> bool {
    let rest = &s[i..];
    let boundary = s[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric() && c != '/' && c != '@' && c != '.');
    boundary && (rest.starts_with("https://") || rest.starts_with("http://") || rest.starts_with("www."))
}

/// Split plain text into `(href, text)` pieces: `href` is `Some` for a bare
/// web address (`https://…`, `http://…`, `www.…`). Punctuation that ends the
/// sentence and an unmatched closing bracket stay outside the link. Pure.
pub fn autolink(text: &str) -> Vec<(Option<String>, String)> {
    let mut out: Vec<(Option<String>, String)> = Vec::new();
    let mut plain_from = 0;
    let mut i = 0;
    while i < text.len() {
        if !text.is_char_boundary(i) || !url_start(text, i) {
            i += 1;
            continue;
        }
        let mut end = text[i..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`'))
            .map_or(text.len(), |n| i + n);
        loop {
            let url = &text[i..end];
            let Some(last) = url.chars().next_back() else { break };
            let unbalanced = |open: char, close: char| url.matches(close).count() > url.matches(open).count();
            let strip = matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*' | '_' | '~')
                || (last == ')' && unbalanced('(', ')'))
                || (last == ']' && unbalanced('[', ']'));
            if !strip {
                break;
            }
            end -= last.len_utf8();
        }
        let url = &text[i..end];
        // "www." or "https://" alone is not an address.
        let host = ["https://", "http://", "www."].iter().find_map(|p| url.strip_prefix(p)).unwrap_or("");
        if !host.contains(|c: char| c.is_alphanumeric()) || (url.starts_with("www.") && !host.contains('.')) {
            i += 1;
            continue;
        }
        if plain_from < i {
            out.push((None, text[plain_from..i].to_string()));
        }
        let href = if url.starts_with("www.") { format!("https://{url}") } else { url.to_string() };
        out.push((Some(href), url.to_string()));
        plain_from = end;
        i = end;
    }
    if plain_from < text.len() {
        out.push((None, text[plain_from..].to_string()));
    }
    out
}

/// `md2pdf`'s argument: `--dark` (also `--dunkel`) as the first or last word
/// selects the dark theme, the rest is the path (spaces kept). Pure.
pub fn split_flags(arg: &str) -> (bool, Option<String>) {
    let is_flag = |w: &str| matches!(w.to_ascii_lowercase().as_str(), "--dark" | "--dunkel");
    let mut rest = arg.trim();
    let mut dark = false;
    loop {
        if let Some((first, tail)) = rest.split_once(char::is_whitespace) {
            if is_flag(first) {
                dark = true;
                rest = tail.trim_start();
                continue;
            }
        }
        if let Some((head, last)) = rest.rsplit_once(char::is_whitespace) {
            if is_flag(last) {
                dark = true;
                rest = head.trim_end();
                continue;
            }
        }
        if is_flag(rest) {
            dark = true;
            rest = "";
        }
        break;
    }
    (dark, (!rest.is_empty()).then(|| rest.to_string()))
}

/// The light document's HTML (tests).
#[cfg(test)]
pub fn render_document(md: &str, base: Option<&Path>) -> String {
    build_document(md, base, false).html
}

pub fn build_document(md: &str, base: Option<&Path>, dark: bool) -> Document {
    use pulldown_cmark::{html, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_MATH);
    options.insert(Options::ENABLE_GFM);
    options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);

    let mut events = Vec::new();
    let mut frontmatter = String::new();
    let mut in_meta = false;
    let mut mermaid: Option<String> = None;
    let (mut has_code, mut has_math, mut has_mermaid) = (false, false, false);
    let mut headings: Vec<Heading> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    // (index of the heading's start event, its text so far)
    let mut open_heading: Option<(usize, String)> = None;
    let (mut link_depth, mut in_code_block) = (0usize, false);

    // Merged text events: a bare address must arrive in one piece.
    for ev in pulldown_cmark::TextMergeStream::new(Parser::new_ext(md, options)) {
        match &ev {
            Event::Start(Tag::Link { .. }) => link_depth += 1,
            Event::End(TagEnd::Link) => link_depth = link_depth.saturating_sub(1),
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Text(t) | Event::Code(t) if !in_meta => {
                if let Some((_, text)) = open_heading.as_mut() {
                    text.push_str(t);
                }
            }
            _ => {}
        }
        match ev {
            Event::Start(Tag::Heading { .. }) => {
                open_heading = Some((events.len(), String::new()));
                events.push(ev);
            }
            Event::End(TagEnd::Heading(level)) => {
                if let Some((at, text)) = open_heading.take() {
                    if let Event::Start(Tag::Heading { id, .. }) = &mut events[at] {
                        // An explicit `{#id}` wins; every heading gets one, so
                        // `[text](#heading)` jumps and the outline has a target.
                        let slug = match id.as_deref() {
                            Some(explicit) => {
                                seen_ids.insert(explicit.to_string());
                                explicit.to_string()
                            }
                            None => unique_slug(&mut seen_ids, slugify(&text)),
                        };
                        *id = Some(slug.clone().into());
                        headings.push(Heading { level: level as u8, title: text.trim().to_string(), id: slug });
                    }
                }
                events.push(ev);
            }
            Event::Text(t) if !in_meta && mermaid.is_none() && link_depth == 0 && !in_code_block => {
                for (href, piece) in autolink(&t) {
                    match href {
                        Some(href) => events.push(Event::InlineHtml(
                            format!("<a href=\"{}\">{}</a>", esc(&href), esc(&piece)).into(),
                        )),
                        None => events.push(Event::Text(piece.into())),
                    }
                }
            }
            Event::Start(Tag::MetadataBlock(_)) => in_meta = true,
            Event::End(TagEnd::MetadataBlock(_)) => in_meta = false,
            Event::Text(t) if in_meta => frontmatter.push_str(&t),
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref lang)))
                if lang.split_whitespace().next() == Some("mermaid") =>
            {
                mermaid = Some(String::new());
            }
            Event::Text(t) if mermaid.is_some() => mermaid.as_mut().unwrap().push_str(&t),
            Event::End(TagEnd::CodeBlock) if mermaid.is_some() => {
                has_mermaid = true;
                let src = mermaid.take().unwrap_or_default();
                events.push(Event::Html(format!("<div class=\"mermaid\">{}</div>\n", esc(&src)).into()));
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref lang))) => {
                if !lang.trim().is_empty() {
                    has_code = true;
                }
                events.push(ev);
            }
            Event::InlineMath(t) => {
                has_math = true;
                events.push(Event::InlineHtml(format!("<span class=\"ir-math\">{}</span>", esc(&t)).into()));
            }
            Event::DisplayMath(t) => {
                has_math = true;
                events.push(Event::InlineHtml(
                    format!("<span class=\"ir-math ir-math-display\">{}</span>", esc(&t)).into(),
                ));
            }
            Event::Start(Tag::BlockQuote(Some(kind))) => {
                let (class, label, icon) = callout_meta(kind);
                events.push(Event::Html(format!(
                    "<div class=\"callout callout-{class}\"><p class=\"callout-title\"><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" class=\"callout-icon\">{icon}</svg><span>{label}</span></p>\n"
                ).into()));
            }
            Event::End(TagEnd::BlockQuote(Some(_))) => events.push(Event::Html("</div>\n".into())),
            Event::Start(Tag::Image { link_type, dest_url, title, id }) => {
                let dest_url = match embed_image(&dest_url, base) {
                    Some(data) => data.into(),
                    None => dest_url,
                };
                events.push(Event::Start(Tag::Image { link_type, dest_url, title, id }));
            }
            other => events.push(other),
        }
    }

    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());
    let fm = parse_frontmatter(&frontmatter);
    let doc_title = fm.get("title").cloned().or_else(|| headings.first().map(|h| h.title.clone())).filter(|t| !t.trim().is_empty());
    let title = doc_title.clone().unwrap_or_else(|| "Markdown".into());
    // One tiny, invisible jump link per heading: WebKit resolves each to a
    // page position in the PDF, which is where the outline entries point
    // (the links themselves are taken out again).
    if !headings.is_empty() {
        body.push_str("<div class=\"ir-outline\" aria-hidden=\"true\">");
        for h in &headings {
            body.push_str(&format!("<a href=\"#{}\">&nbsp;</a>", esc(&h.id)));
        }
        body.push_str("</div>\n");
    }

    let nonce = {
        use rand::Rng;
        let n: u128 = rand::thread_rng().gen();
        format!("{n:032x}")
    };
    let mut css = String::from(MRX_CSS);
    css.push_str(HLJS_CSS);
    css.push_str(EXTRA_CSS);
    if has_math {
        css.push_str(KATEX_CSS);
    }
    if dark {
        css.push_str(DARK_CSS);
    }
    let mut scripts = String::new();
    let mut push_script = |src: &str| {
        // A literal `</script` inside a library would end the tag early.
        scripts.push_str(&format!("<script nonce=\"{nonce}\">{}</script>\n", src.replace("</script", "<\\/script")));
    };
    if has_code {
        push_script(HLJS_JS);
    }
    if has_math {
        push_script(KATEX_JS);
    }
    if has_mermaid {
        push_script(MERMAID_JS);
    }
    push_script(READY_JS);

    let csp = format!(
        "default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:; script-src 'nonce-{nonce}'"
    );
    let html = format!(
        "<!DOCTYPE html>\n<html lang=\"de\" data-theme=\"{theme}\">\n<head>\n<meta charset=\"utf-8\">\n<meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\">\n<title>{title}</title>\n<style>{css}</style>\n</head>\n<body>\n{tp}{body}{scripts}</body>\n</html>\n",
        title = esc(&title),
        tp = title_page(&fm),
        theme = if dark { "dark" } else { "light" },
    );
    Document { html, title: doc_title, author: fm.get("author").cloned().filter(|a| !a.trim().is_empty()), headings, dark }
}

/// Runs after the libraries: highlight, typeset math, draw diagrams, wait for
/// fonts and images — then flag completion. Every step is guarded; a failure
/// leaves that element as plain text instead of blocking the PDF.
const READY_JS: &str = r#"window.__irDone = false;
(async () => {
  try { if (window.hljs) document.querySelectorAll('pre code[class*="language-"]').forEach(el => { try { hljs.highlightElement(el); } catch (e) {} }); } catch (e) {}
  try { if (window.katex) document.querySelectorAll('.ir-math').forEach(el => { try { katex.render(el.textContent, el, { displayMode: el.classList.contains('ir-math-display'), throwOnError: false }); } catch (e) {} }); } catch (e) {}
  try { if (window.mermaid) { mermaid.initialize({ startOnLoad: false, theme: document.documentElement.dataset.theme === 'dark' ? 'dark' : 'neutral', securityLevel: 'strict' }); await mermaid.run({ querySelector: '.mermaid' }); } } catch (e) {}
  try { await document.fonts.ready; } catch (e) {}
  await Promise.all(Array.from(document.images).map(i => i.decode().catch(() => {})));
  window.__irDone = true;
})();"#;

/// Test shorthand (no base folder → relative images aren't embedded).
#[cfg(test)]
pub fn render_html(md: &str) -> String {
    render_document(md, None)
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

// Windows: render via Microsoft Edge headless (`--print-to-pdf`). Edge
// ships with the WebView2 runtime present on all Win10/11.
#[cfg(target_os = "windows")]
fn backend_available() -> bool {
    true
}

// Linux + everything else: no HTML→PDF backend yet.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn backend_available() -> bool {
    false
}

/// Render a self-contained HTML document to PDF. Public so other features
/// can reuse the pipeline instead of growing a second one (`loc`'s export
/// does — one renderer, three formats).
///
/// ⚠️ **Main thread only on macOS** — WebKit asserts it. Dispatch via
/// `app.run_on_main_thread` from a worker.
pub fn html_to_pdf(html: &str, output: &Path) -> Result<(), String> {
    write_pdf(html, output)
}

/// Render a self-contained HTML document to a PNG of the WHOLE page.
///
/// ⚠️ macOS only, main thread only. Elsewhere this reports honestly rather
/// than writing an empty file.
#[cfg(target_os = "macos")]
pub fn html_to_png(html: &str, output: &Path) -> Result<(), String> {
    macos::render_html_to_png(html, output)
}

#[cfg(not(target_os = "macos"))]
pub fn html_to_png(_html: &str, _output: &Path) -> Result<(), String> {
    Err("PNG-Export ist bisher nur auf macOS umgesetzt".into())
}

/// md2pdf's writer: paginated A4 on macOS (WebKit print + page numbers);
/// Windows prints through Edge, which honours mrxdown's `@page` rule.
#[cfg(target_os = "macos")]
fn write_document_pdf(doc: &Document, output: &Path) -> Result<(), String> {
    macos::render_document_pdf_paged(doc, output)
}

#[cfg(not(target_os = "macos"))]
fn write_document_pdf(doc: &Document, output: &Path) -> Result<(), String> {
    write_pdf(&doc.html, output)
}

#[cfg(target_os = "macos")]
fn write_pdf(html: &str, output: &Path) -> Result<(), String> {
    macos::render_html_to_pdf(html, output)
}

#[cfg(target_os = "windows")]
fn write_pdf(html: &str, output: &Path) -> Result<(), String> {
    windows_edge::render_html_to_pdf(html, output)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn write_pdf(_html: &str, _output: &Path) -> Result<(), String> {
    Err(crate::i18n::t("mdpdf.backend_unimpl"))
}

/// HTML → PDF on Windows via Edge headless. Writes the self-contained
/// HTML to a temp file, runs `msedge --headless=new --print-to-pdf`, then
/// removes the temp file. Pure process-spawn (no COM / WebView2 SDK) so
/// it compiles cross-platform; **runtime untested on this build host
/// (macOS) — verify on a real Windows box.**
#[cfg(target_os = "windows")]
mod windows_edge {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub fn render_html_to_pdf(html: &str, output: &Path) -> Result<(), String> {
        let mut tmp = std::env::temp_dir();
        let stem = output.file_stem().and_then(|s| s.to_str()).unwrap_or("md");
        // pid + stem keeps it unique enough for a one-shot conversion
        // without pulling in a rand/uuid dependency.
        tmp.push(format!("inspector-rust-md-{stem}-{}.html", std::process::id()));
        std::fs::write(&tmp, html).map_err(|e| format!("temp HTML write failed: {e}"))?;

        let edge = find_msedge();
        let result = Command::new(&edge)
            .arg("--headless=new")
            .arg("--disable-gpu")
            .arg("--no-pdf-header-footer")
            // highlight.js / KaTeX / Mermaid run in the page before printing.
            .arg("--virtual-time-budget=15000")
            .arg("--run-all-compositor-stages-before-draw")
            .arg(format!("--print-to-pdf={}", output.display()))
            .arg(tmp.display().to_string())
            .status();
        let _ = std::fs::remove_file(&tmp);

        match result {
            Ok(s) if s.success() && output.exists() => Ok(()),
            Ok(s) => Err(format!("msedge exited with {s} and produced no PDF")),
            Err(e) => Err(format!(
                "failed to run Microsoft Edge ({}): {e}",
                edge.display()
            )),
        }
    }

    fn find_msedge() -> PathBuf {
        for c in [
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        ] {
            let p = PathBuf::from(c);
            if p.exists() {
                return p;
            }
        }
        // Last resort: rely on PATH.
        PathBuf::from("msedge.exe")
    }
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
    use objc2::{AnyThread, Encode, Encoding, RefEncode};
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
            // ⚠️ A4, not US-Letter. `createPDFWithConfiguration: nil` takes
            // the page rect from the view's BOUNDS, so this frame IS the
            // paper size — 794×1123 is A4 at 96 DPI. The readers of these
            // reports are on A4; Letter left a stripe at the bottom of every
            // printed page and cropped the right edge.
            let frame = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize { width: 794.0, height: 1123.0 },
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

    // ── Paginated A4 print (md2pdf) ──────────────────────────────────
    //
    // `createPDF` above captures the view as ONE page. md2pdf wants real A4
    // pages, which only WebKit's print path produces. Recipe (prototyped and
    // verified 2026-09-28): an offscreen window hosting the WKWebView, wait for
    // the page's own `window.__irDone`, then `printOperationWithPrintInfo:` run
    // MODALLY for that window — the synchronous `runOperation` deadlocks,
    // because WebKit prints asynchronously. Page numbers are stamped
    // afterwards with PDFKit (WebKit ignores `@page` margin boxes).

    /// A4 in points and mrxdown's margins (20 mm top/bottom, 15 mm sides).
    const A4_W: f64 = 595.2756;
    const A4_H: f64 = 841.8898;
    const MARGIN_TB: f64 = 56.693; // 20 mm
    const MARGIN_LR: f64 = 42.520; // 15 mm

    static PRINT_STATE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

    objc2::define_class!(
        #[unsafe(super(objc2::runtime::NSObject))]
        #[name = "IRMdPrintDelegate"]
        struct PrintDelegate;

        impl PrintDelegate {
            #[unsafe(method(printOperationDidRun:success:contextInfo:))]
            fn did_run(&self, _op: *mut AnyObject, success: objc2::runtime::Bool, _ctx: *mut c_void) {
                PRINT_STATE.store(
                    if success.as_bool() { 1 } else { 2 },
                    std::sync::atomic::Ordering::SeqCst,
                );
            }
        }
    );

    #[link(name = "PDFKit", kind = "framework")]
    extern "C" {}

    /// Render a document from `render_document` to paginated A4 PDF.
    /// **Main thread only.**
    pub fn render_document_pdf_paged(doc: &super::Document, output: &Path) -> Result<(), String> {
        let tmp = std::env::temp_dir().join(format!(
            "inspector-rust-md2pdf-{}-{}.pdf",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let result = unsafe { print_to(&doc.html, &tmp) }.and_then(|()| unsafe { finish_pdf(&tmp, output, doc) });
        let _ = std::fs::remove_file(&tmp);
        result
    }

    unsafe fn print_to(html: &str, out: &Path) -> Result<(), String> {
        // 1) offscreen window + web view (a print operation needs a window
        //    for its modal run, and the view needs real bounds).
        let page = CGRect { origin: CGPoint { x: 0.0, y: 0.0 }, size: CGSize { width: A4_W, height: A4_H } };
        let off = CGRect { origin: CGPoint { x: -20000.0, y: -20000.0 }, size: page.size };
        let win: Id = msg_send![objc2::class!(NSWindow), alloc];
        let win: Id = msg_send![win, initWithContentRect: off, styleMask: 0usize, backing: 2usize, defer: false];
        if win.is_null() {
            return Err("NSWindow init returned nil".into());
        }
        let _: () = msg_send![win, setReleasedWhenClosed: false];
        let config: Id = msg_send![objc2::class!(WKWebViewConfiguration), new];
        let webview: Id = msg_send![objc2::class!(WKWebView), alloc];
        let webview: Id = msg_send![webview, initWithFrame: page, configuration: config];
        release(config);
        if webview.is_null() {
            release(win);
            return Err("WKWebView alloc/init returned nil".into());
        }
        let _: () = msg_send![win, setContentView: webview];
        let cleanup = |webview: Id, win: Id| {
            let _: () = msg_send![win, setContentView: NIL];
            release(webview);
            let _: () = msg_send![win, close];
            release(win);
        };

        // 2) load + wait for the page's own readiness flag.
        let html_ns = nsstring_from_str(html);
        let _: Id = msg_send![webview, loadHTMLString: html_ns, baseURL: NIL];
        release(html_ns);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let loading: bool = msg_send![webview, isLoading];
            if !loading {
                break;
            }
            if Instant::now() > deadline {
                cleanup(webview, win);
                return Err("WKWebView load timed out after 15s".into());
            }
            run_loop_pump(Duration::from_millis(30));
        }
        if !wait_ready(webview, Duration::from_secs(30)) {
            // Print what rendered instead of failing — a stuck diagram must
            // not cost the whole document.
            tracing::warn!("md2pdf: page not ready after 30s — printing as is");
        }
        run_loop_pump(Duration::from_millis(50));

        // 3) print info: A4, mrxdown margins, save to `out`.
        let path_ns = nsstring_from_str(&out.to_string_lossy());
        let url: Id = msg_send![objc2::class!(NSURL), fileURLWithPath: path_ns];
        release(path_ns);
        // ⚠️ The constant NSPrintJobSavingURL's VALUE is "NSJobSavingURL".
        let key = nsstring_from_str("NSJobSavingURL");
        let dict: Id = msg_send![objc2::class!(NSDictionary), dictionaryWithObject: url, forKey: key];
        release(key);
        let pi: Id = msg_send![objc2::class!(NSPrintInfo), alloc];
        let pi: Id = msg_send![pi, initWithDictionary: dict];
        let save = nsstring_from_str("NSPrintSaveJob");
        let _: () = msg_send![pi, setJobDisposition: save];
        release(save);
        let _: () = msg_send![pi, setPaperSize: CGSize { width: A4_W, height: A4_H }];
        let _: () = msg_send![pi, setTopMargin: MARGIN_TB];
        let _: () = msg_send![pi, setBottomMargin: MARGIN_TB];
        let _: () = msg_send![pi, setLeftMargin: MARGIN_LR];
        let _: () = msg_send![pi, setRightMargin: MARGIN_LR];
        let _: () = msg_send![pi, setHorizontalPagination: 1isize]; // fit width
        let _: () = msg_send![pi, setVerticalPagination: 0isize]; // automatic
        let _: () = msg_send![pi, setHorizontallyCentered: false];
        let _: () = msg_send![pi, setVerticallyCentered: false];

        let op: Id = msg_send![webview, printOperationWithPrintInfo: pi];
        release(pi);
        if op.is_null() {
            cleanup(webview, win);
            return Err("printOperationWithPrintInfo returned nil".into());
        }
        let _: () = msg_send![op, setShowsPrintPanel: false];
        let _: () = msg_send![op, setShowsProgressPanel: false];
        let view: Id = msg_send![op, view];
        if !view.is_null() {
            let _: () = msg_send![view, setFrame: page];
        }

        // 4) modal run with a delegate that records the outcome.
        PRINT_STATE.store(0, std::sync::atomic::Ordering::SeqCst);
        let delegate: objc2::rc::Retained<PrintDelegate> = msg_send![PrintDelegate::alloc(), init];
        let _: () = msg_send![op,
            runOperationModalForWindow: win,
            delegate: &*delegate,
            didRunSelector: objc2::sel!(printOperationDidRun:success:contextInfo:),
            contextInfo: std::ptr::null_mut::<c_void>()];
        let deadline = Instant::now() + Duration::from_secs(60);
        let state = loop {
            let s = PRINT_STATE.load(std::sync::atomic::Ordering::SeqCst);
            if s != 0 || Instant::now() > deadline {
                break s;
            }
            run_loop_pump(Duration::from_millis(30));
        };
        drop(delegate);
        cleanup(webview, win);
        match state {
            1 if out.exists() => Ok(()),
            1 => Err("print reported success but wrote no file".into()),
            2 => Err("print operation failed".into()),
            _ => Err("print operation timed out after 60s".into()),
        }
    }

    /// Poll `window.__irDone` (set by the page once highlighting, math,
    /// diagrams, fonts and images are done).
    unsafe fn wait_ready(webview: Id, max: Duration) -> bool {
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let deadline = Instant::now() + max;
        let js = nsstring_from_str("window.__irDone === true");
        let mut ok = false;
        while Instant::now() < deadline {
            let flag = Arc::clone(&done);
            let block = block2::RcBlock::new(move |result: Id, _err: Id| {
                if !result.is_null() {
                    let b: bool = msg_send![result, boolValue];
                    if b {
                        flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            });
            let _: () = msg_send![webview, evaluateJavaScript: js, completionHandler: &*block];
            run_loop_pump(Duration::from_millis(60));
            if done.load(std::sync::atomic::Ordering::SeqCst) {
                ok = true;
                break;
            }
        }
        release(js);
        ok
    }

    // ── Finishing the printed PDF ─────────────────────────────────────
    //
    // Page numbers are DRAWN onto each page while PDFKit writes the file (a
    // `PDFPage` subclass whose draw adds the number). The earlier way — a
    // FreeText annotation per page, burned in with
    // `PDFDocumentBurnInAnnotationsOption` — flattened EVERY annotation, and
    // links are annotations: no link in an md2pdf file was clickable.
    // Measured 2026-11-11: 4 links after printing, 0 after the burn; with
    // the page subclass all survive and the text stays selectable.
    // ⚠️ Links cannot be taken out and put back instead: PDFKit (macOS 27)
    // writes a newly created in-document destination with a broken page
    // reference; only the annotations WebKit wrote survive a rewrite.

    static PAGE_DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// `CGContextRef` for the method signature.
    #[repr(C)]
    struct CGContext {
        _private: [u8; 0],
    }
    unsafe impl RefEncode for CGContext {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGContext", &[]));
    }

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {
        fn NSRectFill(rect: CGRect);
    }

    /// Make `ctx` AppKit's current context for `f`.
    unsafe fn with_graphics_context(ctx: *mut CGContext, f: impl FnOnce()) {
        let gc_class = objc2::class!(NSGraphicsContext);
        let _: () = msg_send![gc_class, saveGraphicsState];
        let gc: Id = msg_send![gc_class, graphicsContextWithCGContext: ctx, flipped: false];
        let _: () = msg_send![gc_class, setCurrentContext: gc];
        f();
        let _: () = msg_send![gc_class, restoreGraphicsState];
    }

    extern "C-unwind" fn numbered_page_draw(this: &AnyObject, _cmd: objc2::runtime::Sel, display_box: isize, ctx: *mut CGContext) {
        unsafe {
            let dark = PAGE_DARK.load(std::sync::atomic::Ordering::SeqCst);
            let b: CGRect = msg_send![this, boundsForBox: 0isize]; // media box
            if dark {
                // WebKit paints the page colour only inside the print
                // margins; the margins themselves get it here.
                let (r, g, bl) = super::DARK_BG;
                with_graphics_context(ctx, || {
                    let c: Id = msg_send![objc2::class!(NSColor),
                        colorWithSRGBRed: r as f64 / 255.0, green: g as f64 / 255.0, blue: bl as f64 / 255.0, alpha: 1.0f64];
                    let _: () = msg_send![c, setFill];
                    NSRectFill(b);
                });
            }
            let _: () = msg_send![super(this, objc2::class!(PDFPage)), drawWithBox: display_box, toContext: ctx];
            let doc: Id = msg_send![this, document];
            if doc.is_null() {
                return;
            }
            let index: usize = msg_send![doc, indexForPage: this];
            with_graphics_context(ctx, || {
                // mrxdown's `@bottom-center` page counter: 9 pt, #999.
                let font: Id = msg_send![objc2::class!(NSFont), systemFontOfSize: 9.0f64];
                let grey: Id = msg_send![objc2::class!(NSColor), colorWithWhite: 0.6f64, alpha: 1.0f64];
                let (kf, kc) = (nsstring_from_str("NSFont"), nsstring_from_str("NSColor"));
                let attrs: Id = msg_send![objc2::class!(NSMutableDictionary), new];
                let _: () = msg_send![attrs, setObject: font, forKey: kf];
                let _: () = msg_send![attrs, setObject: grey, forKey: kc];
                let label = nsstring_from_str(&(index + 1).to_string());
                let size: CGSize = msg_send![label, sizeWithAttributes: attrs];
                let at = CGPoint { x: b.origin.x + (b.size.width - size.width) / 2.0, y: b.origin.y + 24.0 };
                let _: () = msg_send![label, drawAtPoint: at, withAttributes: attrs];
                release(label);
                release(attrs);
                release(kf);
                release(kc);
            });
        }
    }

    extern "C-unwind" fn class_for_page(_this: &AnyObject, _cmd: objc2::runtime::Sel) -> *const objc2::runtime::AnyClass {
        numbered_page_class()
    }

    /// `PDFPage` subclass that draws its page number (registered once).
    fn numbered_page_class() -> &'static objc2::runtime::AnyClass {
        static CLASS: std::sync::OnceLock<&'static objc2::runtime::AnyClass> = std::sync::OnceLock::new();
        CLASS.get_or_init(|| {
            let mut b = objc2::runtime::ClassBuilder::new(c"IRMdNumberedPage", objc2::class!(PDFPage))
                .expect("IRMdNumberedPage registered twice");
            unsafe {
                b.add_method(
                    objc2::sel!(drawWithBox:toContext:),
                    numbered_page_draw as extern "C-unwind" fn(_, _, _, _),
                );
            }
            b.register()
        })
    }

    /// The document delegate that makes PDFKit use the numbered page class.
    fn page_delegate_class() -> &'static objc2::runtime::AnyClass {
        static CLASS: std::sync::OnceLock<&'static objc2::runtime::AnyClass> = std::sync::OnceLock::new();
        CLASS.get_or_init(|| {
            let mut b = objc2::runtime::ClassBuilder::new(c"IRMdPageDelegate", objc2::class!(NSObject))
                .expect("IRMdPageDelegate registered twice");
            unsafe {
                b.add_method(
                    objc2::sel!(classForPage),
                    class_for_page as extern "C-unwind" fn(_, _) -> _,
                );
            }
            b.register()
        })
    }

    /// The printed PDF → the final file: page numbers, the outline built
    /// from the headings, title/author. Links stay as WebKit wrote them.
    unsafe fn finish_pdf(src: &Path, dst: &Path, document: &super::Document) -> Result<(), String> {
        PAGE_DARK.store(document.dark, std::sync::atomic::Ordering::SeqCst);
        let src_ns = nsstring_from_str(&src.to_string_lossy());
        let src_url: Id = msg_send![objc2::class!(NSURL), fileURLWithPath: src_ns];
        release(src_ns);
        let doc: Id = msg_send![objc2::class!(PDFDocument), alloc];
        let doc: Id = msg_send![doc, initWithURL: src_url];
        if doc.is_null() {
            return Err("printed PDF could not be read back".into());
        }
        // Before any page is touched: pages are created through the delegate.
        let delegate: Id = msg_send![page_delegate_class(), new];
        let _: () = msg_send![doc, setDelegate: delegate];

        // The outline anchors: tiny in-document links, one per heading, in
        // document order. Their destinations are the headings' positions.
        let count: usize = msg_send![doc, pageCount];
        let link_type = nsstring_from_str("Link");
        let mut anchors: Vec<Id> = Vec::new(); // retained PDFDestinations
        for i in 0..count {
            let page: Id = msg_send![doc, pageAtIndex: i];
            if page.is_null() {
                continue;
            }
            let annots: Id = msg_send![page, annotations];
            let n: usize = if annots.is_null() { 0 } else { msg_send![annots, count] };
            let mut ours: Vec<Id> = Vec::new();
            for k in 0..n {
                let a: Id = msg_send![annots, objectAtIndex: k];
                let ty: Id = msg_send![a, type];
                let is_link: bool = !ty.is_null() && msg_send![ty, isEqualToString: link_type];
                let bounds: CGRect = msg_send![a, bounds];
                let dest: Id = msg_send![a, destination];
                if is_link && !dest.is_null() && bounds.size.height <= OUTLINE_ANCHOR_MAX_H {
                    let _: Id = msg_send![dest, retain];
                    anchors.push(dest);
                    let _: Id = msg_send![a, retain];
                    ours.push(a);
                }
            }
            for a in ours {
                let _: () = msg_send![page, removeAnnotation: a];
                release(a);
            }
        }
        release(link_type);
        if anchors.len() == document.headings.len() && !anchors.is_empty() {
            let root: Id = msg_send![objc2::class!(PDFOutline), new];
            // (level, item) of the open ancestors.
            let mut stack: Vec<(u8, Id)> = Vec::new();
            for (h, dest) in document.headings.iter().zip(&anchors) {
                let item: Id = msg_send![objc2::class!(PDFOutline), new];
                let label = nsstring_from_str(&h.title);
                let _: () = msg_send![item, setLabel: label];
                release(label);
                let _: () = msg_send![item, setDestination: *dest];
                while stack.last().is_some_and(|(level, _)| *level >= h.level) {
                    stack.pop();
                }
                let parent = stack.last().map_or(root, |(_, p)| *p);
                let at: usize = msg_send![parent, numberOfChildren];
                let _: () = msg_send![parent, insertChild: item, atIndex: at];
                let _: () = msg_send![item, setIsOpen: h.level <= 2];
                stack.push((h.level, item));
                release(item);
            }
            let _: () = msg_send![doc, setOutlineRoot: root];
            release(root);
        } else if !document.headings.is_empty() {
            tracing::warn!("md2pdf: {} headings but {} outline anchors — no outline", document.headings.len(), anchors.len());
        }
        for d in anchors {
            release(d);
        }

        let attrs: Id = msg_send![objc2::class!(NSMutableDictionary), new];
        let put = |key: &str, value: &str| {
            let (k, v) = (nsstring_from_str(key), nsstring_from_str(value));
            let _: () = msg_send![attrs, setObject: v, forKey: k];
            release(k);
            release(v);
        };
        if let Some(t) = &document.title {
            put("Title", t);
        }
        if let Some(a) = &document.author {
            put("Author", a);
        }
        put("Creator", "Inspector Rust");
        let _: () = msg_send![doc, setDocumentAttributes: attrs];
        release(attrs);

        let dst_ns = nsstring_from_str(&dst.to_string_lossy());
        let dst_url: Id = msg_send![objc2::class!(NSURL), fileURLWithPath: dst_ns];
        release(dst_ns);
        let ok: bool = msg_send![doc, writeToURL: dst_url];
        let _: () = msg_send![doc, setDelegate: NIL];
        release(delegate);
        release(doc);
        if ok {
            Ok(())
        } else {
            Err(format!("can't write PDF to {dst:?}"))
        }
    }

    /// What a finished PDF holds, read back through PDFKit (tests).
    #[cfg(test)]
    #[derive(Debug, Default)]
    pub struct PdfFacts {
        pub pages: usize,
        /// Web/mail links, in order.
        pub urls: Vec<String>,
        /// Jumps inside the document: (from page, to page).
        pub jumps: Vec<(usize, usize)>,
        /// Outline entries: (depth, label, page).
        pub outline: Vec<(usize, String, usize)>,
        pub page_text: Vec<String>,
        pub title: Option<String>,
        pub author: Option<String>,
    }

    #[cfg(test)]
    pub fn finish_for_test(src: &Path, dst: &Path, document: &super::Document) -> Result<(), String> {
        unsafe { finish_pdf(src, dst, document) }
    }

    #[cfg(test)]
    pub fn inspect(path: &Path) -> PdfFacts {
        unsafe fn walk(doc: Id, item: Id, depth: usize, out: &mut Vec<(usize, String, usize)>) {
            let n: usize = msg_send![item, numberOfChildren];
            for i in 0..n {
                let child: Id = msg_send![item, childAtIndex: i];
                let label: Id = msg_send![child, label];
                let dest: Id = msg_send![child, destination];
                let page: Id = if dest.is_null() { NIL } else { msg_send![dest, page] };
                let index: usize = if page.is_null() { usize::MAX } else { msg_send![doc, indexForPage: page] };
                out.push((depth, nsstring_to_string(label).unwrap_or_default(), index));
                walk(doc, child, depth + 1, out);
            }
        }
        unsafe {
            let mut f = PdfFacts::default();
            let ns = nsstring_from_str(&path.to_string_lossy());
            let url: Id = msg_send![objc2::class!(NSURL), fileURLWithPath: ns];
            release(ns);
            let doc: Id = msg_send![objc2::class!(PDFDocument), alloc];
            let doc: Id = msg_send![doc, initWithURL: url];
            if doc.is_null() {
                return f;
            }
            f.pages = msg_send![doc, pageCount];
            for i in 0..f.pages {
                let page: Id = msg_send![doc, pageAtIndex: i];
                let text: Id = msg_send![page, string];
                f.page_text.push(nsstring_to_string(text).unwrap_or_default());
                let annots: Id = msg_send![page, annotations];
                let n: usize = msg_send![annots, count];
                for k in 0..n {
                    let a: Id = msg_send![annots, objectAtIndex: k];
                    let u: Id = msg_send![a, URL];
                    let dest: Id = msg_send![a, destination];
                    if !u.is_null() {
                        let abs: Id = msg_send![u, absoluteString];
                        f.urls.push(nsstring_to_string(abs).unwrap_or_default());
                    } else if !dest.is_null() {
                        let dp: Id = msg_send![dest, page];
                        if !dp.is_null() {
                            f.jumps.push((i, msg_send![doc, indexForPage: dp]));
                        }
                    }
                }
            }
            let root: Id = msg_send![doc, outlineRoot];
            if !root.is_null() {
                walk(doc, root, 0, &mut f.outline);
            }
            let attrs: Id = msg_send![doc, documentAttributes];
            if !attrs.is_null() {
                for (key, slot) in [("Title", &mut f.title), ("Author", &mut f.author)] {
                    let k = nsstring_from_str(key);
                    let v: Id = msg_send![attrs, objectForKey: k];
                    release(k);
                    *slot = nsstring_to_string(v);
                }
            }
            release(doc);
            f
        }
    }

    /// An outline anchor link is 1 px high; a real link is a line of text.
    const OUTLINE_ANCHOR_MAX_H: f64 = 3.0;

    /// Iterate the main CFRunLoop for up to `dur`. Returns when one
    /// event is processed OR the timeout elapses — we call this in
    /// a poll loop, not a single big wait.
    /// HTML → PNG of the WHOLE page.
    ///
    /// Same three stages as the PDF path, with one extra step that matters:
    /// `takeSnapshot` captures the view's BOUNDS, not the document, so a
    /// report longer than the initial frame would simply be cut off. We ask
    /// the page for its own height first and resize to it.
    ///
    /// ⚠️ Main thread only, like every WebKit call here.
    pub fn render_html_to_png(html: &str, output: &Path) -> Result<(), String> {
        const WIDTH: f64 = 900.0;
        unsafe {
            let frame = CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize { width: WIDTH, height: 1200.0 },
            };
            let config: Id = msg_send![objc2::class!(WKWebViewConfiguration), new];
            let webview: Id = msg_send![objc2::class!(WKWebView), alloc];
            let webview: Id = msg_send![webview, initWithFrame: frame, configuration: config];
            release(config);
            if webview.is_null() {
                return Err("WKWebView alloc/init returned nil".into());
            }

            let html_nsstring = nsstring_from_str(html);
            let _: Id = msg_send![webview, loadHTMLString: html_nsstring, baseURL: NIL];
            release(html_nsstring);

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
            run_loop_pump(Duration::from_millis(50));

            // ── Measure the document, then grow the view to it ─────────
            // Without this the snapshot is a 1200 px crop of a report that
            // is usually far longer.
            let height = Arc::new(Mutex::new(0.0f64));
            let (htx, hrx) = mpsc::channel::<()>();
            let h_for_block = Arc::clone(&height);
            let htx_for_block = Mutex::new(Some(htx));
            let js_block = block2::RcBlock::new(move |value: Id, _error: Id| {
                if !value.is_null() {
                    let v: f64 = msg_send![value, doubleValue];
                    if let Ok(mut g) = h_for_block.lock() {
                        *g = v;
                    }
                }
                if let Ok(mut s) = htx_for_block.lock() {
                    if let Some(s) = s.take() {
                        let _ = s.send(());
                    }
                }
            });
            // ⚠️ NOT `scrollHeight`: for content shorter than the frame it
            // clamps to the VIEWPORT, so a short report snapshotted with a
            // page of trailing whitespace (measured live: 1200 for a ~400 pt
            // document). The footer is the last element of every report, so
            // its bottom edge is the true content height; scrollHeight stays
            // as the fallback for a document without one.
            let js = nsstring_from_str(
                "(function(){var f=document.querySelector('footer');\
                 if(f){return Math.ceil(f.getBoundingClientRect().bottom+32);}\
                 return Math.ceil(document.body.scrollHeight);})()",
            );
            let _: () = msg_send![webview,
                evaluateJavaScript: js,
                completionHandler: &*js_block];
            release(js);
            let js_deadline = Instant::now() + Duration::from_secs(5);
            while hrx.try_recv().is_err() {
                if Instant::now() > js_deadline {
                    break;
                }
                run_loop_pump(Duration::from_millis(20));
            }
            // A failed measurement falls back to the initial frame rather
            // than to something absurd — a cropped report beats no report.
            let measured = height.lock().map(|g| *g).unwrap_or(0.0);
            tracing::debug!("html_to_png: measured document height = {measured}");
            let full = measured.clamp(200.0, 20_000.0);
            if measured > 0.0 {
                let _: () = msg_send![webview,
                    setFrameSize: CGSize { width: WIDTH, height: full }];
                run_loop_pump(Duration::from_millis(120));
            }

            // ── Snapshot ──────────────────────────────────────────────
            let result: Arc<Mutex<Option<PdfResult>>> = Arc::new(Mutex::new(None));
            let (tx, rx) = mpsc::channel::<()>();
            let result_for_block = Arc::clone(&result);
            let tx_for_block = Mutex::new(Some(tx));
            let block = block2::RcBlock::new(move |image: Id, error: Id| {
                let outcome = if !error.is_null() {
                    let desc: Id = msg_send![error, localizedDescription];
                    PdfResult::Err(
                        nsstring_to_string(desc)
                            .unwrap_or_else(|| "takeSnapshot returned an error".to_string()),
                    )
                } else if image.is_null() {
                    PdfResult::Err("takeSnapshot returned nil".to_string())
                } else {
                    // NSImage → TIFF → NSBitmapImageRep → PNG. There is no
                    // direct NSImage→PNG call; this is the documented route.
                    let tiff: Id = msg_send![image, TIFFRepresentation];
                    if tiff.is_null() {
                        PdfResult::Err("TIFFRepresentation was nil".to_string())
                    } else {
                        let rep: Id =
                            msg_send![objc2::class!(NSBitmapImageRep), imageRepWithData: tiff];
                        if rep.is_null() {
                            PdfResult::Err("NSBitmapImageRep was nil".to_string())
                        } else {
                            let props: Id = msg_send![objc2::class!(NSDictionary), dictionary];
                            // 4 = NSBitmapImageFileTypePNG.
                            let png: Id = msg_send![rep,
                                representationUsingType: 4usize,
                                properties: props];
                            if png.is_null() {
                                PdfResult::Err("PNG encoding returned nil".to_string())
                            } else {
                                let ptr: *const u8 = msg_send![png, bytes];
                                let len: usize = msg_send![png, length];
                                if ptr.is_null() || len == 0 {
                                    PdfResult::Err("PNG data was empty".to_string())
                                } else {
                                    PdfResult::Ok(
                                        std::slice::from_raw_parts(ptr, len).to_vec(),
                                    )
                                }
                            }
                        }
                    }
                };
                if let Ok(mut g) = result_for_block.lock() {
                    *g = Some(outcome);
                }
                if let Ok(mut s) = tx_for_block.lock() {
                    if let Some(s) = s.take() {
                        let _ = s.send(());
                    }
                }
            });
            let _: () = msg_send![webview,
                takeSnapshotWithConfiguration: NIL,
                completionHandler: &*block];

            let snap_deadline = Instant::now() + Duration::from_secs(15);
            loop {
                if rx.try_recv().is_ok() {
                    break;
                }
                if Instant::now() > snap_deadline {
                    release(webview);
                    return Err("takeSnapshot timed out".into());
                }
                run_loop_pump(Duration::from_millis(30));
            }
            release(webview);

            match result.lock().ok().and_then(|mut g| g.take()) {
                Some(PdfResult::Ok(bytes)) => std::fs::write(output, bytes)
                    .map_err(|e| format!("PNG write failed: {e}")),
                Some(PdfResult::Err(e)) => Err(e),
                None => Err("takeSnapshot produced no result".into()),
            }
        }
    }

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
        return crate::i18n::t("mdpdf.no_files");
    }
    if summary.backend_unavailable {
        return crate::i18n::t("mdpdf.unsupported");
    }
    if summary.converted.is_empty() && summary.failed.is_empty() {
        return crate::i18n::tf(
            "mdpdf.no_md",
            &[("n", &summary.skipped.len().to_string())],
        );
    }
    let mut parts = Vec::new();
    if summary.outputs.len() == 1 && summary.failed.is_empty() {
        // One file: name it — the Finder-style " 2" suffix must be visible.
        let name = summary.outputs[0]
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        parts.push(crate::i18n::tf("mdpdf.created_one", &[("name", &name)]));
    } else if !summary.converted.is_empty() {
        parts.push(crate::i18n::tf("mdpdf.created_n", &[("n", &summary.converted.len().to_string())]));
    }
    if !summary.skipped.is_empty() {
        parts.push(crate::i18n::tf("mdpdf.skipped_n", &[("n", &summary.skipped.len().to_string())]));
    }
    if !summary.failed.is_empty() {
        parts.push(crate::i18n::tf("mdpdf.failed_n", &[("n", &summary.failed.len().to_string())]));
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
    fn render_html_wraps_body_with_doctype_and_style() {
        let html = render_html("# Hello\n\nworld");
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<style>"));
        assert!(html.contains("<h1 id=\"hello\">Hello</h1>"));
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
        assert_eq!(build_notification_message(&s), "No files selected");
    }

    #[test]
    fn only_non_md_says_nothing_to_convert() {
        let s = ConvertSummary {
            skipped: vec!["a.png".into(), "b.txt".into()],
            ..Default::default()
        };
        assert_eq!(
            build_notification_message(&s),
            "No Markdown files in the selection (2 skipped)"
        );
    }

    #[test]
    fn success_says_count() {
        let s = ConvertSummary {
            converted: vec!["a.md".into(), "b.md".into()],
            ..Default::default()
        };
        assert_eq!(build_notification_message(&s), "2 PDFs created");
    }

    #[test]
    fn backend_unavailable_message_is_actionable() {
        let s = ConvertSummary {
            backend_unavailable: true,
            failed: vec![("a.md".into(), "backend missing".into())],
            ..Default::default()
        };
        let msg = build_notification_message(&s);
        assert!(msg.contains("not yet supported"));
        assert!(!msg.contains("failed"));
    }

    // ── md2pdf: output naming (Finder "keep both") ──────────────────

    fn taken<'a>(names: &'a [&'a str]) -> impl Fn(&Path) -> bool + 'a {
        move |p: &Path| names.iter().any(|n| p == Path::new(n))
    }

    #[test]
    fn free_pdf_path_keeps_the_name_when_free() {
        assert_eq!(free_pdf_path(Path::new("/d/notes.md"), taken(&[])), PathBuf::from("/d/notes.pdf"));
        assert_eq!(
            free_pdf_path(Path::new("/d/v1.0.markdown"), taken(&[])),
            PathBuf::from("/d/v1.0.pdf")
        );
    }

    #[test]
    fn free_pdf_path_counts_like_the_finder() {
        let p = Path::new("/d/notes.md");
        assert_eq!(free_pdf_path(p, taken(&["/d/notes.pdf"])), PathBuf::from("/d/notes 2.pdf"));
        assert_eq!(
            free_pdf_path(p, taken(&["/d/notes.pdf", "/d/notes 2.pdf"])),
            PathBuf::from("/d/notes 3.pdf")
        );
    }

    #[test]
    fn free_pdf_path_takes_the_first_gap() {
        // `notes 2.pdf` was deleted, `notes 3.pdf` still exists → 2 is free.
        let p = Path::new("/d/notes.md");
        assert_eq!(
            free_pdf_path(p, taken(&["/d/notes.pdf", "/d/notes 3.pdf"])),
            PathBuf::from("/d/notes 2.pdf")
        );
    }

    #[test]
    fn free_pdf_path_keeps_spaces_and_umlauts() {
        let p = Path::new("/d/Mein Bericht Ä.md");
        assert_eq!(
            free_pdf_path(p, taken(&["/d/Mein Bericht Ä.pdf"])),
            PathBuf::from("/d/Mein Bericht Ä 2.pdf")
        );
    }

    // ── md2pdf: which files ─────────────────────────────────────────

    #[test]
    fn a_typed_path_wins_over_the_selection() {
        let got = choose_inputs(
            Some(PathBuf::from("/x/a.md")),
            Ok(vec![PathBuf::from("/y/b.md")]),
            |_| true,
        )
        .unwrap();
        assert_eq!(got, vec![PathBuf::from("/x/a.md")]);
    }

    #[test]
    fn without_a_path_the_selected_markdown_files_are_used() {
        let got = choose_inputs(
            None,
            Ok(vec![PathBuf::from("/y/b.md"), PathBuf::from("/y/c.png"), PathBuf::from("/y/d.markdown")]),
            |_| true,
        )
        .unwrap();
        assert_eq!(got, vec![PathBuf::from("/y/b.md"), PathBuf::from("/y/d.markdown")]);
    }

    #[test]
    fn no_markdown_anywhere_is_an_actionable_error() {
        let e = choose_inputs(None, Ok(vec![PathBuf::from("/y/c.png")]), |_| true).unwrap_err();
        assert!(e.contains("md2pdf <path>"), "{e}");
        let e = choose_inputs(None, Err("osascript".into()), |_| true).unwrap_err();
        assert!(e.contains("No Markdown file"), "{e}");
    }

    #[test]
    fn a_typed_path_must_be_existing_markdown() {
        assert!(choose_inputs(Some(PathBuf::from("/x/a.txt")), Ok(vec![]), |_| true)
            .unwrap_err()
            .contains("Not a Markdown file"));
        assert!(choose_inputs(Some(PathBuf::from("/x/a.md")), Ok(vec![]), |_| false)
            .unwrap_err()
            .contains("not found"));
    }

    // ── md2pdf: mrxdown rendering ───────────────────────────────────

    #[test]
    fn uses_the_mrxdown_stylesheet_and_a_locked_down_csp() {
        let html = render_html("# Hi");
        assert!(html.contains("mrx-titlepage"), "mrxdown default.css must be inlined");
        assert!(html.contains("Content-Security-Policy"));
        assert!(html.contains("default-src 'none'"));
        assert!(html.contains("img-src data:;"), "no network images");
        assert!(html.contains("window.__irDone"), "the renderer waits for this flag");
    }

    #[test]
    fn a_script_in_the_markdown_cannot_run() {
        let html = render_html("<script>alert(1)</script>");
        // Our own scripts carry the nonce; the document's does not.
        let nonce = html.split("'nonce-").nth(1).unwrap().split('\'').next().unwrap();
        let foreign = html.matches("<script>alert(1)").count();
        assert_eq!(foreign, 1);
        assert!(!html.contains(&format!("<script nonce=\"{nonce}\">alert")));
    }

    #[test]
    fn libraries_are_only_shipped_when_the_document_needs_them() {
        let plain = render_html("just text");
        assert!(!plain.contains("hljs.highlightElement(el); }") || !plain.contains("HighlightJS=v"));
        assert!(!plain.contains("HighlightJS=v"), "no highlight.js without code");
        assert!(!plain.contains("katex.min"), "sanity");
        let code = render_html("```rust\nfn main() {}\n```");
        assert!(code.contains("HighlightJS=v"), "highlight.js inlined for code");
        assert!(code.contains("<pre><code class=\"language-rust\""));
    }

    #[test]
    fn math_becomes_katex_targets() {
        let html = render_html("Inline $a^2$ and\n\n$$\\int_0^1 x\\,dx$$\n");
        assert!(html.contains("<span class=\"ir-math\">a^2</span>"));
        assert!(html.contains("ir-math-display"));
        assert!(html.contains("@font-face"), "KaTeX CSS with inlined fonts");
    }

    #[test]
    fn mermaid_fences_become_diagram_divs_with_the_library() {
        let html = render_html("```mermaid\ngraph TD; A-->B\n```");
        assert!(html.contains("<div class=\"mermaid\">graph TD; A--&gt;B\n</div>"));
        assert!(!html.contains("language-mermaid"));
        assert!(html.contains("mermaid.run"));
    }

    #[test]
    fn gfm_alerts_become_mrxdown_callouts() {
        let html = render_html("> [!WARNING]\n> Careful");
        assert!(html.contains("<div class=\"callout callout-warning\">"));
        assert!(html.contains("<span>Warnung</span>"));
        assert!(html.contains("Careful"));
        // A plain quote stays a quote.
        assert!(render_html("> quote").contains("<blockquote>"));
    }

    #[test]
    fn frontmatter_becomes_a_title_page_and_is_not_printed_raw() {
        let md = "---\ntitle: \"Bericht\"\nauthor: Martin\ndate: 2026-09-28\n---\n\n# Kapitel\n";
        let html = render_html(md);
        assert!(html.contains("<div class=\"mrx-titlepage\"><h1>Bericht</h1>"));
        assert!(html.contains("<div class=\"mrx-author\">Martin</div>"));
        assert!(html.contains("<title>Bericht</title>"));
        assert!(!html.contains("author: Martin"), "raw YAML must not reach the body");
        // No title → no title page.
        assert!(!render_html("---\nauthor: x\n---\n\ntext").contains("<div class=\"mrx-titlepage\">"));
    }

    #[test]
    fn parse_frontmatter_reads_flat_key_values() {
        let fm = parse_frontmatter("title: 'A: B'\nsubtitle: x\n  nested: no\n# comment: no\nempty:\n");
        assert_eq!(fm.get("title").map(String::as_str), Some("A: B"));
        assert_eq!(fm.get("subtitle").map(String::as_str), Some("x"));
        assert!(!fm.contains_key("nested"));
        assert!(!fm.contains_key("empty"));
    }

    #[test]
    fn local_images_are_embedded_remote_ones_left_alone() {
        let dir = std::env::temp_dir().join(format!("ir-md2pdf-img-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("img dir")).unwrap();
        std::fs::write(dir.join("img dir/p.png"), [0x89, b'P', b'N', b'G']).unwrap();
        let html = render_document("![a](img%20dir/p.png) ![b](https://example.com/x.png) ![c](missing.png)", Some(&dir));
        assert!(html.contains("src=\"data:image/png;base64,iVBORw==\""), "relative + %20 path embedded");
        assert!(html.contains("src=\"https://example.com/x.png\""), "remote untouched (CSP blocks it)");
        assert!(html.contains("src=\"missing.png\""), "missing stays as written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn percent_decoding_is_byte_safe() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%C3%A4"), "ä");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("ä%2"), "ä%2");
    }

    #[test]
    fn vendored_scripts_cannot_close_their_script_tag() {
        for (name, src) in [("hljs", HLJS_JS), ("katex", KATEX_JS), ("mermaid", MERMAID_JS)] {
            assert!(!src.contains("</script"), "{name} contains </script");
        }
    }

    /// Writes the HTML of `$IR_MD_IN` to `$IR_DUMP_DIR/md2pdf.html`, to look
    /// at: `IR_MD_IN=doc.md IR_DUMP_DIR=/tmp/x cargo test --lib dump_md2pdf -- --ignored`.
    #[test]
    #[ignore = "writes a file to look at"]
    fn dump_md2pdf_html() {
        let (Ok(input), Ok(dir)) = (std::env::var("IR_MD_IN"), std::env::var("IR_DUMP_DIR")) else { return };
        let input = PathBuf::from(input);
        let md = std::fs::read_to_string(&input).unwrap();
        let dark = std::env::var_os("IR_MD_DARK").is_some();
        std::fs::write(Path::new(&dir).join("md2pdf.html"), build_document(&md, input.parent(), dark).html).unwrap();
    }

    /// The Markdown behind `fixtures/md2pdf-printed.pdf` (what WebKit's print
    /// step wrote for it, before finishing). Regenerate the PDF with the
    /// dump test above and a WKWebView print when this changes.
    const FIXTURE_MD: &str = "---\ntitle: Prüfdokument\nauthor: Test Autor\n---\n\n# Einleitung\n\nEin [externer Link](https://example.com/pfad?x=1), nackt https://www.example.org/seite und <mailto:hallo@example.com>. Sprung zum [Schluss](#schluss).\n\n## Unterpunkt\n\nText.\n\n<div style=\"height:1400px\"></div>\n\n# Schluss\n\nZurück zur [Einleitung](#einleitung).\n";

    #[test]
    #[ignore = "writes the fixture's HTML"]
    fn dump_md2pdf_fixture_html() {
        let Ok(dir) = std::env::var("IR_DUMP_DIR") else { return };
        std::fs::write(Path::new(&dir).join("fixture.html"), build_document(FIXTURE_MD, None, false).html).unwrap();
    }

    /// The finishing step on a real printed PDF: every link survives, the
    /// headings become the outline, each page carries its number.
    #[cfg(target_os = "macos")]
    #[test]
    fn finishing_keeps_links_and_adds_outline_numbers_and_properties() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/md2pdf-printed.pdf");
        let dir = std::env::temp_dir().join(format!("ir-md2pdf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for dark in [false, true] {
            let out = dir.join(format!("out-{dark}.pdf"));
            let doc = build_document(FIXTURE_MD, None, dark);
            macos::finish_for_test(&src, &out, &doc).unwrap();
            let f = macos::inspect(&out);
            assert_eq!(f.pages, 3, "{f:?}");
            assert_eq!(
                f.urls,
                vec!["https://example.com/pfad?x=1", "https://www.example.org/seite", "mailto:hallo@example.com"],
                "web and mail links"
            );
            // Einleitung → Schluss and back; the outline's own anchors are gone.
            assert_eq!(f.jumps, vec![(1, 2), (2, 1)]);
            assert_eq!(
                f.outline,
                vec![(0, "Einleitung".to_string(), 1), (1, "Unterpunkt".to_string(), 1), (0, "Schluss".to_string(), 2)]
            );
            for (i, text) in f.page_text.iter().enumerate() {
                assert_eq!(text.trim().lines().last().map(str::trim), Some((i + 1).to_string().as_str()), "page number on page {i}: {text:?}");
            }
            assert!(f.page_text[1].contains("externer Link"), "text stays text");
            assert!(!f.page_text[2].contains('.') || f.page_text[2].contains("Einleitung."), "no helper dots in the text layer");
            assert_eq!(f.page_text[2].trim().lines().count(), 3, "heading, sentence, number: {:?}", f.page_text[2]);
            assert_eq!((f.title.as_deref(), f.author.as_deref()), (Some("Prüfdokument"), Some("Test Autor")));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn headings_get_github_anchors() {
        assert_eq!(slugify("Code & Mathe"), "code--mathe");
        assert_eq!(slugify("  Über uns: Größe_1  "), "über-uns-größe_1");
        assert_eq!(slugify("!!!"), "section");
        let mut seen = std::collections::HashSet::new();
        assert_eq!(unique_slug(&mut seen, "setup".into()), "setup");
        assert_eq!(unique_slug(&mut seen, "setup".into()), "setup-1");
        assert_eq!(unique_slug(&mut seen, "setup".into()), "setup-2");

        let d = build_document("# Start\n\n## Mit `code` drin\n\n## Start\n\n[hoch](#start)", None, false);
        assert!(d.html.contains("<h1 id=\"start\">"), "{}", &d.html[d.html.len() - 600..]);
        assert!(d.html.contains("<h2 id=\"mit-code-drin\">"));
        assert!(d.html.contains("<h2 id=\"start-1\">"));
        assert_eq!(
            d.headings.iter().map(|h| (h.level, h.title.as_str(), h.id.as_str())).collect::<Vec<_>>(),
            vec![(1, "Start", "start"), (2, "Mit code drin", "mit-code-drin"), (2, "Start", "start-1")]
        );
        // One invisible jump link per heading, for the outline.
        assert!(d.html.contains("<div class=\"ir-outline\" aria-hidden=\"true\"><a href=\"#start\">&nbsp;</a><a href=\"#mit-code-drin\">&nbsp;</a><a href=\"#start-1\">&nbsp;</a></div>"));
        assert_eq!(d.title.as_deref(), Some("Start"));
        // No headings, no block.
        assert!(!build_document("nur Text", None, false).html.contains("class=\"ir-outline\""));
    }

    #[test]
    fn bare_addresses_become_links() {
        let l = |s: &str| autolink(s).into_iter().filter_map(|(h, _)| h).collect::<Vec<_>>();
        assert_eq!(l("siehe https://example.com/a?b=1&c=2."), vec!["https://example.com/a?b=1&c=2"]);
        assert_eq!(l("(www.example.org/x) und http://a.de, fertig"), vec!["https://www.example.org/x", "http://a.de"]);
        assert_eq!(l("https://de.wikipedia.org/wiki/Rust_(Programmiersprache)"), vec!["https://de.wikipedia.org/wiki/Rust_(Programmiersprache)"]);
        // Not addresses: a word that merely contains one, a bare prefix, mail.
        assert!(l("xhttps://a.de foo@www.example.com www. https:// www.x").is_empty());
        // Text around the link is kept, in order.
        assert_eq!(
            autolink("a https://x.de b"),
            vec![(None, "a ".to_string()), (Some("https://x.de".to_string()), "https://x.de".to_string()), (None, " b".to_string())]
        );
        assert_eq!(autolink("größe ändern"), vec![(None, "größe ändern".to_string())]);

        let html = render_html("Nackt https://example.com/pfad_mit_unterstrich und [schon](https://example.com/y) Link.\n\n`https://code.example`\n\n```\nhttps://block.example\n```");
        assert!(html.contains("<a href=\"https://example.com/pfad_mit_unterstrich\">https://example.com/pfad_mit_unterstrich</a>"));
        // A written link is not linked twice, code stays code.
        assert_eq!(html.matches("href=\"https://example.com/y\"").count(), 1);
        assert!(!html.contains("href=\"https://code.example"));
        assert!(!html.contains("href=\"https://block.example"));
    }

    #[test]
    fn the_dark_flag_is_split_from_the_path() {
        assert_eq!(split_flags("--dark ~/Mein Ordner/a b.md"), (true, Some("~/Mein Ordner/a b.md".into())));
        assert_eq!(split_flags("~/x.md --dark"), (true, Some("~/x.md".into())));
        assert_eq!(split_flags("--DARK"), (true, None));
        assert_eq!(split_flags("  --dunkel  "), (true, None));
        assert_eq!(split_flags("~/x.md"), (false, Some("~/x.md".into())));
        assert_eq!(split_flags(""), (false, None));
        // Only as a word of its own.
        assert_eq!(split_flags("~/notes--dark.md"), (false, Some("~/notes--dark.md".into())));
        assert_eq!(split_flags("~/a --dark b.md"), (false, Some("~/a --dark b.md".into())));
    }

    #[test]
    fn dark_documents_carry_the_dark_sheet_light_ones_do_not() {
        let light = build_document("# A", None, false);
        let dark = build_document("# A", None, true);
        assert!(dark.dark && dark.html.contains("data-theme=\"dark\"") && dark.html.contains("background: #0d1117 !important"));
        assert!(!light.dark && light.html.contains("data-theme=\"light\"") && !light.html.contains("#0d1117"));
        assert_eq!(DARK_BG, (0x0d, 0x11, 0x17));
    }

    #[test]
    fn title_and_author_come_from_the_frontmatter() {
        let d = build_document("---\ntitle: Bericht\nauthor: A. Autorin\n---\n\n# Kapitel", None, false);
        assert_eq!((d.title.as_deref(), d.author.as_deref()), (Some("Bericht"), Some("A. Autorin")));
        assert_eq!(build_document("ohne alles", None, false).title, None);
    }

    #[test]
    fn a_single_output_is_named_in_the_notification() {
        let s = ConvertSummary {
            converted: vec![PathBuf::from("/d/notes.md")],
            outputs: vec![PathBuf::from("/d/notes 2.pdf")],
            ..Default::default()
        };
        assert_eq!(build_notification_message(&s), "notes 2.pdf created");
    }
}
