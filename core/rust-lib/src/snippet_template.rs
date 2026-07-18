//! Dynamic snippet placeholders (v0.50.0+).
//!
//! Snippet bodies may embed `{…}` placeholders that are expanded at *paste*
//! time. The body is stored verbatim (placeholders intact) and rendered on
//! every paste path — popup search-paste, the abbreviation hotkey's AX
//! in-place / selection / clipboard sub-paths, and direct hotkey slots —
//! because rendering happens at the leaf paste primitives, not in the UI.
//!
//! Supported tokens:
//!
//! | Token                   | Expands to                                  |
//! |-------------------------|---------------------------------------------|
//! | `{date}` / `{date:FMT}` | current date — default `%Y-%m-%d`, FMT = strftime |
//! | `{time}` / `{time:FMT}` | current time — default `%H:%M`              |
//! | `{datetime[:FMT]}`      | default `%Y-%m-%d %H:%M`                     |
//! | `{clipboard}`           | the clipboard text at paste time (`""` if none) |
//! | `{cursor}`              | removed; the caret is left here after pasting |
//! | `{{` and `}}`           | a literal `{` and `}`                       |
//!
//! An unknown `{token}` is emitted **verbatim** (braces included) so a body
//! that legitimately contains `{foo}` is never silently eaten. A `{date:…}`
//! with a malformed strftime spec likewise falls back to verbatim rather
//! than panicking.
//!
//! This module is pure (time + clipboard are injected) → fully unit-tested.

use chrono::{DateTime, Local};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The body with every recognised placeholder substituted.
    pub text: String,
    /// How many chars (Unicode scalar values) to move the caret LEFT from
    /// the end of `text` after pasting — derived from the first `{cursor}`
    /// marker. `0` leaves the caret at the end (the common case).
    pub cursor_back: usize,
}

/// Cheap pre-check: does `body` contain anything worth rendering? Lets hot
/// paste paths skip the work (and the caret move) when there's no `{`.
#[allow(dead_code)]
pub fn has_placeholders(body: &str) -> bool {
    body.contains('{')
}

enum Token {
    /// Replace with this literal text.
    Text(String),
    /// The `{cursor}` marker — leaves no text, records the caret position.
    Cursor,
    /// Unrecognised — re-emit the original `{token}` unchanged.
    Verbatim,
}

/// Expand `body`'s placeholders against the supplied `now` and `clipboard`.
pub fn render(body: &str, now: DateTime<Local>, clipboard: Option<&str>) -> Rendered {
    let mut out = String::with_capacity(body.len());
    // Char-position of the first `{cursor}` within `out` (counted lazily).
    let mut cursor_pos: Option<usize> = None;
    // Per-render faker state (fresh seed + `#label` cache), created lazily on
    // the first `{faker:…}` so a template with none pays nothing. The command's
    // `--seed` never reaches here — this RNG is seeded from OS entropy.
    let mut faker_ctx: Option<crate::faker::FakerCtx> = None;
    let mut iter = body.chars().peekable();

    while let Some(c) = iter.next() {
        match c {
            '{' if iter.peek() == Some(&'{') => {
                iter.next();
                out.push('{');
            }
            '}' if iter.peek() == Some(&'}') => {
                iter.next();
                out.push('}');
            }
            '{' => {
                // Collect up to the next '}'.
                let mut token = String::new();
                let mut closed = false;
                for nc in iter.by_ref() {
                    if nc == '}' {
                        closed = true;
                        break;
                    }
                    token.push(nc);
                }
                if !closed {
                    // Unterminated `{…` — emit it verbatim.
                    out.push('{');
                    out.push_str(&token);
                    continue;
                }
                match expand_token(&token, &now, clipboard, &mut faker_ctx) {
                    Token::Text(s) => out.push_str(&s),
                    Token::Cursor => {
                        if cursor_pos.is_none() {
                            cursor_pos = Some(out.chars().count());
                        }
                        // Extra `{cursor}` markers are dropped silently.
                    }
                    Token::Verbatim => {
                        out.push('{');
                        out.push_str(&token);
                        out.push('}');
                    }
                }
            }
            _ => out.push(c),
        }
    }

    let cursor_back = cursor_pos
        .map(|p| out.chars().count().saturating_sub(p))
        .unwrap_or(0);
    Rendered { text: out, cursor_back }
}

fn expand_token(
    token: &str,
    now: &DateTime<Local>,
    clipboard: Option<&str>,
    faker_ctx: &mut Option<crate::faker::FakerCtx>,
) -> Token {
    // `name` is the part before an optional `:FMT`. We trim only the name —
    // the format string is taken verbatim (a leading space could be intended).
    let (name, fmt) = match token.split_once(':') {
        Some((n, f)) => (n.trim(), Some(f)),
        None => (token.trim(), None),
    };

    let timed = |default: &str| match format_time(now, fmt.unwrap_or(default)) {
        Some(s) => Token::Text(s),
        None => Token::Verbatim, // malformed strftime → leave untouched
    };

    match name {
        "date" => timed("%Y-%m-%d"),
        "time" => timed("%H:%M"),
        "datetime" => timed("%Y-%m-%d %H:%M"),
        "clipboard" | "clip" => Token::Text(clipboard.unwrap_or("").to_string()),
        "cursor" => Token::Cursor,
        // `{faker:<gen>[:args][@locale][#label]}` — fresh per expansion, same
        // value for the same (spec, #label). Unknown generator → left verbatim.
        "faker" => {
            let spec = fmt.unwrap_or("");
            let ctx = faker_ctx.get_or_insert_with(|| {
                crate::faker::FakerCtx::new(crate::faker::process_default_locale())
            });
            match crate::faker::expand_faker(spec, ctx) {
                Some(s) => Token::Text(s),
                None => Token::Verbatim,
            }
        }
        _ => Token::Verbatim,
    }
}

/// Format `now` with a strftime string, returning `None` (so the caller can
/// fall back to verbatim) if the spec is malformed — never panicking.
fn format_time(now: &DateTime<Local>, fmt: &str) -> Option<String> {
    use chrono::format::{Item, StrftimeItems};
    let items: Vec<_> = StrftimeItems::new(fmt).collect();
    if items.iter().any(|i| matches!(i, Item::Error)) {
        return None;
    }
    Some(now.format_with_items(items.iter()).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed() -> DateTime<Local> {
        // 2026-06-05 14:30:07 local.
        Local.with_ymd_and_hms(2026, 6, 5, 14, 30, 7).unwrap()
    }

    fn r(body: &str, clip: Option<&str>) -> Rendered {
        render(body, fixed(), clip)
    }

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(r("hello world", None).text, "hello world");
        assert_eq!(r("hello world", None).cursor_back, 0);
    }

    #[test]
    fn date_and_time_defaults() {
        assert_eq!(r("{date}", None).text, "2026-06-05");
        assert_eq!(r("{time}", None).text, "14:30");
        assert_eq!(r("{datetime}", None).text, "2026-06-05 14:30");
    }

    #[test]
    fn custom_strftime_format() {
        assert_eq!(r("{date:%d.%m.%Y}", None).text, "05.06.2026");
        assert_eq!(r("{time:%H:%M:%S}", None).text, "14:30:07");
    }

    #[test]
    fn clipboard_token() {
        assert_eq!(r("re: {clipboard}", Some("the subject")).text, "re: the subject");
        // No clipboard → empty substitution, not the literal token.
        assert_eq!(r("[{clipboard}]", None).text, "[]");
    }

    #[test]
    fn cursor_marker_is_removed_and_offset_recorded() {
        let out = r("Dear {cursor},\nthanks", None);
        assert_eq!(out.text, "Dear ,\nthanks");
        // chars after the caret: ",\nthanks" == 8.
        assert_eq!(out.cursor_back, 8);
    }

    #[test]
    fn cursor_at_end_is_zero_offset() {
        let out = r("type here {cursor}", None);
        assert_eq!(out.text, "type here ");
        assert_eq!(out.cursor_back, 0);
    }

    #[test]
    fn only_first_cursor_counts_extras_dropped() {
        let out = r("a{cursor}b{cursor}c", None);
        assert_eq!(out.text, "abc");
        // caret after "a" → "bc" remains == 2.
        assert_eq!(out.cursor_back, 2);
    }

    #[test]
    fn escaped_braces_are_literal() {
        assert_eq!(r("use {{date}} like this", None).text, "use {date} like this");
        assert_eq!(r("{{}}", None).text, "{}");
    }

    #[test]
    fn unknown_token_is_verbatim() {
        assert_eq!(r("hi {name}", None).text, "hi {name}");
        assert_eq!(r("{nope:x}", None).text, "{nope:x}");
    }

    #[test]
    fn malformed_strftime_falls_back_to_verbatim() {
        // `%Q` is not a valid strftime specifier.
        assert_eq!(r("{date:%Q}", None).text, "{date:%Q}");
    }

    #[test]
    fn unterminated_brace_is_verbatim() {
        assert_eq!(r("oops {date", None).text, "oops {date");
    }

    #[test]
    fn cursor_offset_counts_unicode_chars_not_bytes() {
        // "ümlaut" after the caret is 6 chars (ü is 2 bytes, 1 char).
        let out = r("{cursor}ümlaut", None);
        assert_eq!(out.text, "ümlaut");
        assert_eq!(out.cursor_back, 6);
    }

    #[test]
    fn has_placeholders_detects_braces() {
        assert!(has_placeholders("a {date} b"));
        assert!(!has_placeholders("no braces here"));
    }

    // ── Faker placeholder integration ────────────────────────────────────────

    #[test]
    fn faker_placeholder_expands_non_empty() {
        let out = r("Hallo {faker:first_name},", None);
        assert!(out.text.starts_with("Hallo "));
        assert!(out.text.ends_with(","));
        // Something was substituted (not left literal).
        assert!(!out.text.contains("{faker"));
        assert!(out.text.len() > "Hallo ,".len());
    }

    #[test]
    fn faker_unknown_generator_stays_literal() {
        assert_eq!(r("{faker:bogus_gen}", None).text, "{faker:bogus_gen}");
        // Bare `{faker}` with no spec stays literal too.
        assert_eq!(r("{faker}", None).text, "{faker}");
    }

    #[test]
    fn faker_label_binding_same_value_within_one_expansion() {
        // Same #label ⇒ identical value in greeting + signature.
        let out = r("{faker:first_name#k} … {faker:first_name#k}", None);
        let parts: Vec<&str> = out.text.split(" … ").collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], parts[1], "same label must repeat the value");
    }

    #[test]
    fn faker_escaped_braces_stay_literal() {
        assert_eq!(r("{{faker:first_name}}", None).text, "{faker:first_name}");
    }

    #[test]
    fn faker_cursor_offset_is_correct_after_variable_length_value() {
        // The cursor is placed AFTER the faker value; cursor_back counts from
        // the true end of the fully-rendered string (0 here — cursor at end).
        let out = r("x {faker:uuid}{cursor}", None);
        assert_eq!(out.cursor_back, 0);
        // uuid is 36 chars → total length is "x " + 36.
        assert_eq!(out.text.chars().count(), 2 + 36);
    }

    #[test]
    fn faker_is_freshly_seeded_across_renders() {
        // Two separate renders almost surely produce different uuids.
        assert_ne!(r("{faker:uuid}", None).text, r("{faker:uuid}", None).text);
    }
}
