//! Smart "Artist - Track (Edition)" naming for social downloads (v0.129.0).
//!
//! yt-dlp's default template produces `Title [videoid].ext`. This module turns
//! the available metadata into a clean library-style name:
//!
//!  1. **Music metadata first** — when YouTube supplies `artist` + `track`
//!     (YouTube Music / Content-ID), those are the cleanest source.
//!  2. **Title split** — otherwise `Artist - Track` is parsed from the title
//!     (first ` - ` / ` – ` / ` — ` / ` | `).
//!  3. **Uploader fallback** — no separator → the cleaned channel name is the
//!     artist (`CyndiLauperVEVO` → `Cyndi Lauper`? no — glued names only lose
//!     the `VEVO`; ` - Topic` / trailing `Official` are stripped).
//!
//! Bracket groups in the title are CLASSIFIED, not copied: junk (`Official
//! Video`, `Lyric Video`, `4K`, `HD`, `Visualizer`, …) is dropped, meaningful
//! editions (`Live in Paris`, `Acoustic`, `Remastered`, `feat. X`, a year, …)
//! are kept — and a mixed group like `(Official Live Video)` keeps only its
//! meaningful words → `(Live)`. A kept `Remaster` without a year picks up
//! `release_year`. Everything is pure + exhaustively unit-tested; the impure
//! rename (collision-safe) lives with the caller in `social_dl.rs`.

/// The metadata a download run yields (empty string = absent).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaMeta {
    pub artist: String,
    pub track: String,
    pub title: String,
    pub uploader: String,
    pub release_year: String,
}

/// Words that never belong in a filename (promo/format noise).
const JUNK_WORDS: &[&str] = &[
    "official", "video", "videos", "audio", "lyric", "lyrics", "lyrical", "visualizer",
    "visualiser", "hd", "hq", "4k", "8k", "1080p", "720p", "60fps", "mv", "m/v", "promo",
    "premiere", "musikvideo", "videoclip", "clip",
];

/// Indicators that a bracket group carries a real edition worth keeping.
const KEEP_WORDS: &[&str] = &[
    "live", "acoustic", "unplugged", "remix", "remaster", "remastered", "version", "edit",
    "mix", "demo", "cover", "session", "sessions", "instrumental", "orchestral", "symphonic",
    "piano", "feat", "feat.", "ft", "ft.", "featuring", "radio", "extended", "deluxe", "mono",
    "stereo", "tour", "concert", "festival", "sped", "slowed", "reverb", "mashup", "medley",
    "reprise", "redux", "rework", "rerecorded", "re-recorded", "single", "bonus", "reissue",
];

fn is_year(w: &str) -> bool {
    w.len() == 4 && (w.starts_with("19") || w.starts_with("20")) && w.chars().all(|c| c.is_ascii_digit())
}

fn norm_word(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.' && c != '-')
        .to_lowercase()
}

/// Classify one bracket group's CONTENT: `None` = drop, `Some(text)` = keep
/// (junk words removed, so `Official Live Video` survives as `Live`).
pub fn classify_group(content: &str) -> Option<String> {
    let words: Vec<&str> = content.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    // "music" is junk only next to video/audio/official — `(Music Box Version)`
    // must survive intact.
    let has_av = words.iter().any(|w| {
        let n = norm_word(w);
        n == "video" || n == "audio" || n == "official"
    });
    let kept: Vec<&str> = words
        .iter()
        .filter(|w| {
            let n = norm_word(w);
            !(JUNK_WORDS.contains(&n.as_str()) || (has_av && n == "music"))
        })
        .copied()
        .collect();
    let meaningful = kept
        .iter()
        .any(|w| {
            let n = norm_word(w);
            KEEP_WORDS.contains(&n.as_str()) || is_year(&n)
        });
    if !meaningful || kept.is_empty() {
        return None;
    }
    Some(kept.join(" "))
}

/// Split a title into its bracket-free core + the raw bracket contents, in
/// order. `()` and `[]` both count; nesting isn't (titles don't nest).
pub fn split_brackets(title: &str) -> (String, Vec<String>) {
    let mut core = String::new();
    let mut groups = Vec::new();
    let mut cur = String::new();
    let mut depth = 0u32;
    for c in title.chars() {
        match c {
            '(' | '[' if depth == 0 => depth = 1,
            ')' | ']' if depth == 1 => {
                depth = 0;
                let g = cur.trim().to_string();
                if !g.is_empty() {
                    groups.push(g);
                }
                cur.clear();
            }
            _ if depth > 0 => cur.push(c),
            _ => core.push(c),
        }
    }
    if depth > 0 && !cur.trim().is_empty() {
        // Unclosed bracket — treat the tail as a group rather than losing it.
        groups.push(cur.trim().to_string());
    }
    let core = core.split_whitespace().collect::<Vec<_>>().join(" ");
    (core, groups)
}

/// Strip channel decorations from an uploader name: a ` - Topic` suffix
/// (auto-generated music channels), trailing `VEVO` (also glued:
/// `CyndiLauperVEVO`), trailing `Official` / `Music` words.
pub fn clean_uploader(uploader: &str) -> String {
    let mut u = uploader.trim().to_string();
    for suffix in [" - topic", " – topic"] {
        if u.to_lowercase().ends_with(suffix) {
            u.truncate(u.len() - suffix.len());
        }
    }
    // Glued or spaced VEVO tail (always uppercase on real channels).
    if let Some(stripped) = u.strip_suffix("VEVO") {
        u = stripped.trim_end().to_string();
    }
    loop {
        let lower = u.to_lowercase();
        let Some(word) = lower.split_whitespace().last() else { break };
        if (word == "official" || word == "music") && lower.split_whitespace().count() > 1 {
            u.truncate(u.len() - word.len());
            u = u.trim_end().to_string();
        } else {
            break;
        }
    }
    u.trim().trim_end_matches(['-', '–']).trim().to_string()
}

/// The first `Artist - Track` separator in a bracket-free title core.
fn split_artist_track(core: &str) -> Option<(String, String)> {
    for sep in [" - ", " – ", " — ", " | "] {
        if let Some(i) = core.find(sep) {
            let (a, t) = (core[..i].trim(), core[i + sep.len()..].trim());
            if !a.is_empty() && !t.is_empty() {
                return Some((a.to_string(), t.to_string()));
            }
        }
    }
    None
}

/// Make a string safe as a file STEM (no extension): path separators and
/// colons become dashes, control chars vanish, whitespace collapses, and the
/// result is capped at 150 chars on a char boundary.
pub fn sanitize_stem(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '/' | '\\' => out.push('-'),
            ':' | '|' => out.push('-'),
            '"' | '*' | '?' | '<' | '>' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let mut collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 150 {
        collapsed = collapsed.chars().take(150).collect::<String>().trim_end().to_string();
    }
    collapsed.trim_matches(|c: char| c == '.' || c == ' ' || c == '-').to_string()
}

/// The smart file stem for a download. `None` when there's nothing usable
/// (caller keeps the original name then).
pub fn smart_stem(meta: &MediaMeta) -> Option<String> {
    let (core, raw_groups) = split_brackets(&meta.title);
    let mut editions: Vec<String> = raw_groups.iter().filter_map(|g| classify_group(g)).collect();

    // A kept remaster without a year picks up the release year.
    if !meta.release_year.trim().is_empty() {
        for e in &mut editions {
            let has_year = e.split_whitespace().any(|w| is_year(&norm_word(w)));
            if !has_year && e.to_lowercase().contains("remaster") {
                e.push(' ');
                e.push_str(meta.release_year.trim());
            }
        }
    }

    let (artist, track) = if !meta.artist.trim().is_empty() && !meta.track.trim().is_empty() {
        // Music metadata is the cleanest source; editions still come from the
        // title, but skip any already contained in the track name.
        let track = meta.track.trim().to_string();
        editions.retain(|e| !track.to_lowercase().contains(&e.to_lowercase()));
        (meta.artist.trim().to_string(), track)
    } else if let Some((a, t)) = split_artist_track(&core) {
        (a, t)
    } else {
        let a = clean_uploader(&meta.uploader);
        let t = core.trim().to_string();
        if t.is_empty() {
            return None;
        }
        if a.is_empty() {
            let mut stem = t;
            for e in &editions {
                stem.push_str(&format!(" ({e})"));
            }
            let s = sanitize_stem(&stem);
            return if s.is_empty() { None } else { Some(s) };
        }
        (a, t)
    };

    // Don't repeat the artist inside the track ("Artist - Artist - Track").
    let track = match track.strip_prefix(&format!("{artist} - ")) {
        Some(rest) => rest.to_string(),
        None => track,
    };

    let mut stem = format!("{artist} - {track}");
    let mut seen: Vec<String> = Vec::new();
    for e in editions {
        let key = e.to_lowercase();
        if seen.contains(&key) || stem.to_lowercase().contains(&key) {
            continue;
        }
        seen.push(key);
        stem.push_str(&format!(" ({e})"));
    }
    let s = sanitize_stem(&stem);
    if s.is_empty() { None } else { Some(s) }
}

/// Strip a yt-dlp ` [videoid]` tail from a stem (8–16 chars of the id
/// alphabet) — the minimum cleanup when no metadata arrived.
pub fn strip_id_suffix(stem: &str) -> String {
    let t = stem.trim_end();
    if let Some(open) = t.rfind(" [") {
        let inner = &t[open + 2..];
        if let Some(id) = inner.strip_suffix(']') {
            let ok_len = (8..=16).contains(&id.len());
            let ok_chars = id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if ok_len && ok_chars {
                return t[..open].trim_end().to_string();
            }
        }
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(artist: &str, track: &str, title: &str, uploader: &str, year: &str) -> MediaMeta {
        MediaMeta {
            artist: artist.into(),
            track: track.into(),
            title: title.into(),
            uploader: uploader.into(),
            release_year: year.into(),
        }
    }

    #[test]
    fn the_field_report_case_keeps_the_live_edition() {
        // "Cyndi Lauper - Girls Just Want to Have Fun (Live in Paris) [id].m4a"
        let m = meta(
            "",
            "",
            "Cyndi Lauper - Girls Just Want to Have Fun (Live in Paris)",
            "CyndiLauperVEVO",
            "",
        );
        assert_eq!(
            smart_stem(&m).as_deref(),
            Some("Cyndi Lauper - Girls Just Want to Have Fun (Live in Paris)")
        );
    }

    #[test]
    fn junk_brackets_are_dropped() {
        for junk in ["(Official Music Video)", "[Official Video]", "(Official Audio)", "(Lyric Video)", "[4K]", "(HD)", "(Visualizer)"] {
            let m = meta("", "", &format!("Rick Astley - Never Gonna Give You Up {junk}"), "", "");
            assert_eq!(smart_stem(&m).as_deref(), Some("Rick Astley - Never Gonna Give You Up"), "{junk}");
        }
    }

    #[test]
    fn a_mixed_group_keeps_only_its_meaningful_words() {
        assert_eq!(classify_group("Official Live Video"), Some("Live".into()));
        assert_eq!(classify_group("Official 4K Remaster"), Some("Remaster".into()));
        assert_eq!(classify_group("Official Video"), None);
        assert_eq!(classify_group("Napalm Records"), None); // no keeper word → drop
        // "music" is junk only NEXT TO video/audio/official.
        assert_eq!(classify_group("Music Box Version"), Some("Music Box Version".into()));
    }

    #[test]
    fn music_metadata_wins_and_title_editions_still_attach() {
        let m = meta("Queen", "Bohemian Rhapsody", "Bohemian Rhapsody (Remastered 2011)", "Queen - Topic", "");
        assert_eq!(smart_stem(&m).as_deref(), Some("Queen - Bohemian Rhapsody (Remastered 2011)"));
        // An edition already inside the track name isn't duplicated.
        let m2 = meta("A", "Song (Live)", "Song (Live) [Official]", "", "");
        assert_eq!(smart_stem(&m2).as_deref(), Some("A - Song (Live)"));
    }

    #[test]
    fn remaster_without_year_picks_up_the_release_year() {
        let m = meta("", "", "Queen - Bohemian Rhapsody (Remastered)", "", "2011");
        assert_eq!(smart_stem(&m).as_deref(), Some("Queen - Bohemian Rhapsody (Remastered 2011)"));
        // With a year already there, nothing is injected.
        let m2 = meta("", "", "Queen - Bohemian Rhapsody (Remastered 2011)", "", "1975");
        assert_eq!(smart_stem(&m2).as_deref(), Some("Queen - Bohemian Rhapsody (Remastered 2011)"));
    }

    #[test]
    fn uploader_fallback_cleans_channel_decorations() {
        assert_eq!(clean_uploader("Queen - Topic"), "Queen");
        assert_eq!(clean_uploader("CyndiLauperVEVO"), "CyndiLauper");
        assert_eq!(clean_uploader("Rick Astley Official"), "Rick Astley");
        assert_eq!(clean_uploader("Depeche Mode"), "Depeche Mode"); // "Mode" ≠ suffix noise
        let m = meta("", "", "Some Great Song (Live)", "Queen - Topic", "");
        assert_eq!(smart_stem(&m).as_deref(), Some("Queen - Some Great Song (Live)"));
    }

    #[test]
    fn feat_groups_and_year_groups_are_editions() {
        let m = meta("", "", "Artist - Song (feat. Nicki Minaj) (Official Video)", "", "");
        assert_eq!(smart_stem(&m).as_deref(), Some("Artist - Song (feat. Nicki Minaj)"));
        let m2 = meta("", "", "Artist - Song (1987)", "", "");
        assert_eq!(smart_stem(&m2).as_deref(), Some("Artist - Song (1987)"));
    }

    #[test]
    fn separators_and_artist_dedupe() {
        let m = meta("", "", "Artist – Track", "", "");
        assert_eq!(smart_stem(&m).as_deref(), Some("Artist - Track"));
        // First separator wins; the rest stays in the track.
        let m2 = meta("", "", "A - B - C", "", "");
        assert_eq!(smart_stem(&m2).as_deref(), Some("A - B - C"));
        // Music-metadata track that already starts with the artist.
        let m3 = meta("Artist", "Artist - Track", "x", "", "");
        assert_eq!(smart_stem(&m3).as_deref(), Some("Artist - Track"));
    }

    #[test]
    fn sanitize_kills_path_hostiles_and_caps_length() {
        assert_eq!(sanitize_stem("AC/DC - Back In Black"), "AC-DC - Back In Black");
        assert_eq!(sanitize_stem("Title: Subtitle?"), "Title- Subtitle");
        let long = "x".repeat(400);
        assert!(sanitize_stem(&long).chars().count() <= 150);
        let m = meta("", "", "AC/DC - Thunderstruck (Live)", "", "");
        assert_eq!(smart_stem(&m).as_deref(), Some("AC-DC - Thunderstruck (Live)"));
    }

    #[test]
    fn no_usable_metadata_returns_none_and_id_suffix_strips() {
        assert_eq!(smart_stem(&meta("", "", "", "", "")), None);
        // Title-only, no uploader → the cleaned title alone.
        let m = meta("", "", "Just A Vlog (Official Video)", "", "");
        assert_eq!(smart_stem(&m).as_deref(), Some("Just A Vlog"));
        assert_eq!(strip_id_suffix("Song [iS61LkCObFc]"), "Song");
        assert_eq!(strip_id_suffix("Song [not an id!]"), "Song [not an id!]");
        assert_eq!(strip_id_suffix("Song [ab]"), "Song [ab]"); // too short
        assert_eq!(strip_id_suffix("Song"), "Song");
    }

    #[test]
    fn unclosed_bracket_does_not_lose_the_tail() {
        let (core, groups) = split_brackets("Artist - Song (Live at Wembley");
        assert_eq!(core, "Artist - Song");
        assert_eq!(groups, vec!["Live at Wembley".to_string()]);
    }
}
