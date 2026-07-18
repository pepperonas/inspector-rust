//! Font registry + resolution for the FIGlet engine.
//!
//! **Commit 1 (this file's initial form):** exposes only the four `figlet-rs`
//! built-in fonts, so the engine + tests work before the compressed bundle
//! exists. **Commit 2** replaces the internals with the lazily-decompressed
//! hundreds-of-fonts bundle (embedded deflate blobs + a curated
//! category/popular overlay) while keeping this exact public API:
//!
//! - [`standard`] — the always-available default `FIGlet` (Arc-shared).
//! - [`load`] — resolve a font by name (cached; `None` if unknown).
//! - [`catalog`] — font metadata for the frontend (never the bytes).

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use figlet_rs::FIGlet;
use parking_lot::Mutex;

use super::FontMeta;

/// The built-in fonts available before the bundle lands. `(name, ctor)`.
type Builtin = (&'static str, fn() -> Result<FIGlet, String>);
const BUILTINS: &[Builtin] = &[
    ("standard", FIGlet::standard),
    ("slant", FIGlet::slant),
    ("small", FIGlet::small),
    ("big", FIGlet::big),
];

/// Parsed-font cache — a font is parsed at most once, then Arc-shared.
fn cache() -> &'static Mutex<HashMap<String, Arc<FIGlet>>> {
    static C: OnceLock<Mutex<HashMap<String, Arc<FIGlet>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The default font — always parseable, so this never fails.
pub fn standard() -> Arc<FIGlet> {
    load("standard").expect("the standard built-in font must always parse")
}

/// Resolve a font by name, caching the parsed result. `None` for an unknown
/// name (the engine falls back to [`standard`]).
pub fn load(name: &str) -> Option<Arc<FIGlet>> {
    if let Some(f) = cache().lock().get(name) {
        return Some(Arc::clone(f));
    }
    let ctor = BUILTINS.iter().find(|(n, _)| *n == name)?.1;
    let font = Arc::new(ctor().ok()?);
    cache().lock().insert(name.to_string(), Arc::clone(&font));
    Some(font)
}

/// Category assigned to each built-in (the curated overlay replaces this in
/// commit 2).
fn category_of(name: &str) -> &'static str {
    match name {
        "slant" => "slanted",
        "small" => "small",
        "big" => "block",
        _ => "standard",
    }
}

/// Font metadata for the gallery — never includes font bytes.
pub fn catalog() -> Vec<FontMeta> {
    BUILTINS
        .iter()
        .map(|(name, _)| FontMeta {
            name: (*name).to_string(),
            category: category_of(name).to_string(),
            popular: true,
            pinned: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_font_loads_and_renders() {
        let f = standard();
        let out = f.convert("Hi").map(|x| x.to_string()).unwrap_or_default();
        assert!(!out.trim().is_empty());
    }

    #[test]
    fn load_caches_same_arc() {
        let a = load("standard").unwrap();
        let b = load("standard").unwrap();
        assert!(Arc::ptr_eq(&a, &b), "second load returns the cached Arc");
    }

    #[test]
    fn unknown_font_is_none() {
        assert!(load("no-such-font").is_none());
    }

    #[test]
    fn catalog_lists_the_builtins() {
        let cat = catalog();
        assert!(cat.iter().any(|m| m.name == "standard" && m.category == "standard"));
        assert!(cat.iter().any(|m| m.name == "slant" && m.category == "slanted"));
        assert_eq!(cat.len(), BUILTINS.len());
    }
}
