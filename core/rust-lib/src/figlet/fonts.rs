//! Font registry + resolution for the FIGlet engine — the lazily-decompressed
//! bundle.
//!
//! Hundreds of `.flf` fonts are embedded **gzip-compressed** (build.rs
//! generates the `FONT_BLOBS` table below), so the binary carries only ~1 MB
//! and nothing is inflated at startup. A font is inflated + parsed **on first
//! use** and then `Arc`-shared from a cache (never inflated twice). Metadata
//! (name/category/popular) is a separate committed overlay
//! (`figlet-fonts.categories.json`) — the frontend receives only that, never
//! the font bytes.
//!
//! Public API (stable since commit 1): [`standard`] · [`load`] · [`catalog`].

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, OnceLock};

use figlet_rs::FIGlet;
use parking_lot::Mutex;
use serde::Deserialize;

use super::FontMeta;

// The generated `FONT_BLOBS: &[(&str, &[u8])]` — (font name, gzip bytes).
include!(concat!(env!("OUT_DIR"), "/figlet_fonts_generated.rs"));

/// Curated metadata overlay: category + popular flag per font. Single Source of
/// Truth for the gallery grouping (built alongside the bundle).
const CATEGORIES_JSON: &str = include_str!("../../assets/figlet-fonts.categories.json");

#[derive(Deserialize)]
struct CatEntry {
    name: String,
    category: String,
    popular: bool,
}

fn categories() -> &'static Vec<CatEntry> {
    static C: OnceLock<Vec<CatEntry>> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str(CATEGORIES_JSON).unwrap_or_default())
}

/// Parsed-font cache — a font is inflated + parsed at most once, then shared.
fn cache() -> &'static Mutex<HashMap<String, Arc<FIGlet>>> {
    static C: OnceLock<Mutex<HashMap<String, Arc<FIGlet>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Inflate a gzip blob to the raw `.flf` text.
fn inflate(gz: &[u8]) -> Option<String> {
    let mut out = String::new();
    flate2::read::GzDecoder::new(gz).read_to_string(&mut out).ok()?;
    Some(out)
}

/// The default font — always present in the bundle, so this never fails (it
/// falls back to the figlet-rs built-in if the bundle is somehow missing it).
pub fn standard() -> Arc<FIGlet> {
    load("standard").unwrap_or_else(|| {
        Arc::new(FIGlet::standard().expect("figlet-rs standard built-in must parse"))
    })
}

/// Resolve a font by name — inflate + parse on first use, then Arc-cached.
/// `None` for an unknown name (the engine falls back to [`standard`]).
pub fn load(name: &str) -> Option<Arc<FIGlet>> {
    if let Some(f) = cache().lock().get(name) {
        return Some(Arc::clone(f));
    }
    let gz = FONT_BLOBS.iter().find(|(n, _)| *n == name)?.1;
    let text = inflate(gz)?;
    let font = Arc::new(FIGlet::from_content(&text).ok()?);
    cache().lock().insert(name.to_string(), Arc::clone(&font));
    Some(font)
}

/// Font metadata for the gallery — never includes font bytes. `pinned` is
/// always `false` here; the IPC layer merges the user's pinned set from
/// settings. Fonts present in the bundle but missing from the overlay default
/// to category `other`.
pub fn catalog() -> Vec<FontMeta> {
    let cats: HashMap<&str, &CatEntry> =
        categories().iter().map(|c| (c.name.as_str(), c)).collect();
    FONT_BLOBS
        .iter()
        .map(|(name, _)| {
            let meta = cats.get(name);
            FontMeta {
                name: (*name).to_string(),
                category: meta.map(|m| m.category.clone()).unwrap_or_else(|| "other".into()),
                popular: meta.map(|m| m.popular).unwrap_or(false),
                pinned: false,
            }
        })
        .collect()
}

/// Number of bundled fonts (test diagnostic).
#[cfg(test)]
pub fn count() -> usize {
    FONT_BLOBS.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_has_hundreds_of_fonts() {
        assert!(count() >= 500, "expected the full set, got {}", count());
    }

    #[test]
    fn every_font_inflates_parses_and_renders_nonempty() {
        // Over the WHOLE set — catches a compressed/broken/mis-encoded font.
        // Non-empty is the universal correctness bar; some fonts are legitimately
        // single-row (binary/morse/hex/term/…), so multi-line is checked below
        // on the popular set, which is all multi-row.
        let mut broken = Vec::new();
        for (name, _) in FONT_BLOBS {
            let Some(font) = load(name) else {
                broken.push(format!("{name}: failed to inflate/parse"));
                continue;
            };
            let out = font.convert("Hello").map(|f| f.to_string()).unwrap_or_default();
            if out.trim().is_empty() {
                broken.push(format!("{name}: empty render"));
            }
        }
        assert!(broken.is_empty(), "broken fonts:\n{}", broken.join("\n"));
    }

    #[test]
    fn popular_fonts_render_multiline() {
        for m in catalog().into_iter().filter(|m| m.popular) {
            let font = load(&m.name).unwrap();
            let out = font.convert("Hello").map(|f| f.to_string()).unwrap_or_default();
            assert!(
                out.lines().count() >= 2,
                "popular font {} should be multi-line",
                m.name
            );
        }
    }

    #[test]
    fn lazy_inflate_is_byte_identical_to_a_fresh_inflate() {
        // The cached parse must equal a fresh direct inflate+parse of the blob.
        let (name, gz) = FONT_BLOBS.iter().find(|(n, _)| *n == "standard").unwrap();
        let via_load = load(name).unwrap().convert("Test").unwrap().to_string();
        let fresh = FIGlet::from_content(&inflate(gz).unwrap())
            .unwrap()
            .convert("Test")
            .unwrap()
            .to_string();
        assert_eq!(via_load, fresh);
    }

    #[test]
    fn load_caches_same_arc_no_double_inflate() {
        let a = load("slant").unwrap();
        let b = load("slant").unwrap();
        assert!(Arc::ptr_eq(&a, &b), "second load must return the cached Arc");
    }

    #[test]
    fn unknown_font_is_none() {
        assert!(load("no-such-font-xyz").is_none());
    }

    #[test]
    fn catalog_covers_every_blob_and_flags_popular() {
        let cat = catalog();
        assert_eq!(cat.len(), count());
        // A known popular font is flagged + categorised.
        let std = cat.iter().find(|m| m.name == "standard").unwrap();
        assert!(std.popular);
        assert_eq!(std.category, "standard");
        assert!(cat.iter().filter(|m| m.popular).count() >= 20);
        // No blob is left without metadata (category always set).
        assert!(cat.iter().all(|m| !m.category.is_empty()));
    }
}
