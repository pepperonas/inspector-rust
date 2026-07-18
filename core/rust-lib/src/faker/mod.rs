//! Realistic fake-data generation for the `faker` search-bar command and the
//! `{faker:…}` snippet-expander placeholder (v0.84.270).
//!
//! One seeded engine (`rand010::StdRng`) drives both, so a given seed always
//! reproduces the same output. The [`registry`] is the single source of truth
//! for the generator catalogue; [`locale`] carries the honest per-generator
//! locale support (fake silently inherits EN, so we surface fallbacks).

mod locale;
mod registry;

pub use locale::Locale;

use rand010::rngs::StdRng;
use rand010::{RngExt, SeedableRng};
use registry::{gen_one, Args};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_N: u32 = 10_000;

/// A non-deterministic StdRng (seeded from the thread RNG) for samples /
/// per-expansion faker state — rand 0.10 has no `StdRng::from_os_rng`.
fn fresh_rng() -> StdRng {
    StdRng::seed_from_u64(rand010::rng().random())
}

/// Process-wide default locale for `{faker:…}` snippet placeholders, so the
/// Settings default-locale applies to the expander too (not just the command).
/// Stored as an index into `Locale::ALL`; seeded at startup + on settings save.
static DEFAULT_LOCALE_IDX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1); // DeDe

pub fn process_default_locale() -> Locale {
    let i = DEFAULT_LOCALE_IDX.load(std::sync::atomic::Ordering::Relaxed);
    Locale::ALL.get(i).copied().unwrap_or(Locale::DeDe)
}

pub fn set_process_default_locale(l: Locale) {
    if let Some(i) = Locale::ALL.iter().position(|x| *x == l) {
        DEFAULT_LOCALE_IDX.store(i, std::sync::atomic::Ordering::Relaxed);
    }
}

// ── Settings defaults (persisted per-field in the settings table) ────────────

/// Persisted faker defaults (mirrors `bruno`'s per-field settings rows).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FakerDefaults {
    /// Canonical locale code (e.g. `DE_DE`).
    pub locale: String,
    pub count: u32,
    pub format: String,
    pub pinned: Vec<String>,
    pub save_history: bool,
}

impl Default for FakerDefaults {
    fn default() -> Self {
        FakerDefaults {
            locale: Locale::DeDe.code().to_string(),
            count: 1,
            format: "plain".to_string(),
            pinned: Vec::new(),
            save_history: true,
        }
    }
}

impl FakerDefaults {
    pub fn locale(&self) -> Locale {
        Locale::from_code(&self.locale).unwrap_or(Locale::DeDe)
    }
}

const KEY_LOCALE: &str = "faker.locale";
const KEY_COUNT: &str = "faker.count";
const KEY_FORMAT: &str = "faker.format";
const KEY_PINNED: &str = "faker.pinned";
const KEY_SAVE_HISTORY: &str = "faker.save_history";

const FORMATS: &[&str] = &["plain", "json", "csv", "sql", "ts"];

/// Read the persisted defaults (each field its own `faker.<x>` settings row),
/// falling back to [`FakerDefaults::default`] per field. Mirrors `bruno`.
pub fn get_defaults(db: &crate::db::DbHandle) -> anyhow::Result<FakerDefaults> {
    let d = FakerDefaults::default();
    let locale = crate::settings::get_or(db, KEY_LOCALE, &d.locale)?;
    let locale = if Locale::from_code(&locale).is_some() { locale } else { d.locale };
    let count = crate::settings::get_or(db, KEY_COUNT, &d.count.to_string())?
        .parse::<u32>()
        .unwrap_or(d.count)
        .clamp(1, MAX_N);
    let format = crate::settings::get_or(db, KEY_FORMAT, &d.format)?;
    let format = if FORMATS.contains(&format.as_str()) { format } else { d.format };
    let pinned = crate::settings::get_or(db, KEY_PINNED, "")?;
    let pinned: Vec<String> = pinned
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && registry::lookup(s).is_some())
        .collect();
    let save_history = crate::settings::get_bool(db, KEY_SAVE_HISTORY, d.save_history)?;
    Ok(FakerDefaults { locale, count, format, pinned, save_history })
}

/// Persist the defaults (whitelisted/clamped), one settings row per field.
pub fn set_defaults(db: &crate::db::DbHandle, d: &FakerDefaults) -> anyhow::Result<()> {
    let locale = if Locale::from_code(&d.locale).is_some() {
        d.locale.clone()
    } else {
        Locale::DeDe.code().to_string()
    };
    let format = if FORMATS.contains(&d.format.as_str()) {
        d.format.clone()
    } else {
        "plain".to_string()
    };
    let pinned: Vec<&str> = d
        .pinned
        .iter()
        .map(|s| s.as_str())
        .filter(|s| registry::lookup(s).is_some())
        .collect();
    crate::settings::set(db, KEY_LOCALE, &locale)?;
    crate::settings::set(db, KEY_COUNT, &d.count.clamp(1, MAX_N).to_string())?;
    crate::settings::set(db, KEY_FORMAT, &format)?;
    crate::settings::set(db, KEY_PINNED, &pinned.join(","))?;
    crate::settings::set(db, KEY_SAVE_HISTORY, if d.save_history { "true" } else { "false" })?;
    // Keep the process-wide expander default in sync with the setting.
    if let Some(l) = Locale::from_code(&locale) {
        set_process_default_locale(l);
    }
    Ok(())
}

/// Seed the process-wide expander default locale from settings at startup.
pub fn init_process_default(db: &crate::db::DbHandle) {
    if let Ok(d) = get_defaults(db) {
        set_process_default_locale(d.locale());
    }
}

// ── Catalogue ────────────────────────────────────────────────────────────────

/// One catalogue row for the frontend (fetched once via `faker_catalog`).
#[derive(Debug, Clone, Serialize)]
pub struct CatalogEntry {
    pub name: String,
    pub aliases: Vec<String>,
    pub category: String,
    pub description: String,
    pub supported_locales: Vec<String>,
    /// A freshly-generated sample in the default locale (for the live preview).
    pub sample: String,
    pub composite: bool,
    pub numeric: bool,
    pub fields: Vec<String>,
}

/// A selectable locale for the Settings dropdown (code + display label).
#[derive(Debug, Clone, Serialize)]
pub struct LocaleOption {
    pub code: String,
    pub label: String,
}

/// All 14 locales fake ships, for the Settings default-locale picker.
pub fn locales() -> Vec<LocaleOption> {
    Locale::ALL
        .iter()
        .map(|l| LocaleOption { code: l.code().to_string(), label: l.label().to_string() })
        .collect()
}

/// Build the catalogue with a live sample per generator in `default` locale.
pub fn catalog(default: Locale) -> Vec<CatalogEntry> {
    let mut rng = fresh_rng();
    registry::CATALOG
        .iter()
        .map(|g| {
            let (eff, _) = locale::resolve(default, g.supported);
            let sample = gen_one(g.name, &mut rng, eff, &Args::default())
                .map(value_to_sample)
                .unwrap_or_default();
            CatalogEntry {
                name: g.name.to_string(),
                aliases: g.aliases.iter().map(|s| s.to_string()).collect(),
                category: g.category.to_string(),
                description: g.description.to_string(),
                supported_locales: g.supported.iter().map(|l| l.code().to_string()).collect(),
                sample,
                composite: g.composite,
                numeric: g.numeric,
                fields: g.fields.iter().map(|s| s.to_string()).collect(),
            }
        })
        .collect()
}

fn value_to_sample(v: Value) -> String {
    match v {
        Value::String(s) => s.replace('\n', " · "),
        Value::Object(_) => v.to_string(),
        other => other.to_string(),
    }
}

// ── Generation ───────────────────────────────────────────────────────────────

/// A generation request (from the command or a `tpl` invocation).
#[derive(Debug, Clone, Deserialize)]
pub struct GenRequest {
    pub generator: String,
    pub n: u32,
    pub locale: Option<String>,
    pub seed: Option<u64>,
    pub args: Option<String>,
    /// When set, `generator` is ignored and each row renders this template.
    pub template: Option<String>,
}

/// The generated payload (raw values; formatting happens in the frontend).
#[derive(Debug, Clone, Serialize)]
pub struct GenResult {
    pub values: Vec<Value>,
    pub seed: u64,
    pub locale_used: String,
    pub fell_back: bool,
    pub generator: String,
    pub fields: Option<Vec<String>>,
}

/// Generate all values in one call. Deterministic for a given `seed`; without
/// one a random seed is chosen and returned so it can be pinned afterwards.
pub fn generate(req: &GenRequest, default: Locale) -> Result<GenResult, String> {
    let n = req.n.clamp(1, MAX_N);
    if req.n > MAX_N {
        return Err(format!("n too large (max {MAX_N})"));
    }
    let seed = req.seed.unwrap_or_else(|| rand010::rng().random());
    let mut rng = StdRng::seed_from_u64(seed);
    let requested = req
        .locale
        .as_deref()
        .and_then(Locale::from_code)
        .unwrap_or(default);

    // Template mode: render each row against the template.
    if let Some(tpl) = &req.template {
        let mut values = Vec::with_capacity(n as usize);
        for _ in 0..n {
            values.push(Value::String(render_template(tpl, &mut rng, requested)));
        }
        return Ok(GenResult {
            values,
            seed,
            locale_used: requested.code().to_string(),
            fell_back: false,
            generator: "tpl".to_string(),
            fields: None,
        });
    }

    let spec = registry::lookup(&req.generator)
        .ok_or_else(|| format!("unknown generator '{}'", req.generator))?;
    let (eff, fell_back) = locale::resolve(requested, spec.supported);
    let args = Args::new(req.args.clone());
    let mut values = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let v = gen_one(spec.name, &mut rng, eff, &args)
            .ok_or_else(|| format!("unknown generator '{}'", spec.name))?;
        values.push(v);
    }
    Ok(GenResult {
        values,
        seed,
        locale_used: eff.code().to_string(),
        fell_back,
        generator: spec.name.to_string(),
        fields: if spec.fields.is_empty() {
            None
        } else {
            Some(spec.fields.iter().map(|s| s.to_string()).collect())
        },
    })
}

// ── Template rendering (`faker tpl "…"`) ─────────────────────────────────────

/// Render `{gen}` / `{gen:args}` / `{gen@locale}` placeholders in a template.
/// `{{` / `}}` are literal braces; an unknown placeholder is left verbatim.
fn render_template(tpl: &str, rng: &mut StdRng, default: Locale) -> String {
    let mut out = String::with_capacity(tpl.len());
    let mut chars = tpl.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut tok = String::new();
                let mut closed = false;
                for d in chars.by_ref() {
                    if d == '}' {
                        closed = true;
                        break;
                    }
                    tok.push(d);
                }
                if !closed {
                    out.push('{');
                    out.push_str(&tok);
                } else {
                    let mut labels = std::collections::HashMap::new();
                    match expand_placeholder(&tok, rng, default, &mut labels) {
                        Some(s) => out.push_str(&s),
                        None => {
                            out.push('{');
                            out.push_str(&tok);
                            out.push('}');
                        }
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

// ── Expander integration (`{faker:…}` in snippets) ───────────────────────────

/// Per-expansion faker state: a fresh RNG (so each expansion is freshly seeded,
/// never the command's `--seed`) plus the `#label` value cache.
pub struct FakerCtx {
    rng: StdRng,
    labels: std::collections::HashMap<(String, String), String>,
    default: Locale,
}

impl FakerCtx {
    pub fn new(default: Locale) -> Self {
        FakerCtx {
            rng: fresh_rng(),
            labels: std::collections::HashMap::new(),
            default,
        }
    }
}

impl Default for FakerCtx {
    fn default() -> Self {
        FakerCtx::new(Locale::DeDe)
    }
}

/// Expand the part of a `{faker:<spec>}` placeholder after the `faker:` prefix.
/// `spec` = `generator[:args][@locale][#label]`. Same value for the same
/// `(spec-without-label, label)` within one expansion; unknown generator →
/// `None` (caller keeps the placeholder literal). Fresh RNG per `FakerCtx`, so
/// the command's `--seed` never reaches this path.
pub fn expand_faker(spec: &str, ctx: &mut FakerCtx) -> Option<String> {
    // Disjoint field borrows: rng + labels are separate fields of ctx.
    expand_placeholder(spec, &mut ctx.rng, ctx.default, &mut ctx.labels)
}

/// A parsed faker placeholder body.
struct ParsedSpec {
    generator: String,
    args: Option<String>,
    locale: Option<Locale>,
    label: Option<String>,
}

fn parse_spec(body: &str) -> ParsedSpec {
    // Order-tolerant: strip #label, then @locale, the first ':' splits args.
    let (rest, label) = match body.split_once('#') {
        Some((a, b)) => (a.to_string(), Some(b.to_string())),
        None => (body.to_string(), None),
    };
    let (rest, locale) = match rest.split_once('@') {
        Some((a, b)) => (a.to_string(), Locale::from_flag(b)),
        None => (rest, None),
    };
    let (generator, args) = match rest.split_once(':') {
        Some((g, a)) => (g.trim().to_string(), Some(a.to_string())),
        None => (rest.trim().to_string(), None),
    };
    ParsedSpec { generator, args, locale, label }
}

/// Shared placeholder expander for template mode (`{gen}`) and the snippet
/// path (`{faker:gen}`, prefix already stripped by the caller). `labels` caches
/// `#label` values per `(spec, label)` for the current expansion.
fn expand_placeholder(
    token: &str,
    rng: &mut StdRng,
    default: Locale,
    labels: &mut std::collections::HashMap<(String, String), String>,
) -> Option<String> {
    let body = token.strip_prefix("faker:").unwrap_or(token);
    let p = parse_spec(body);
    let spec = registry::lookup(&p.generator)?;
    let loc = p.locale.unwrap_or(default);
    let (eff, _) = locale::resolve(loc, spec.supported);
    if let Some(label) = &p.label {
        let key = (format!("{}:{:?}:{}", p.generator, p.args, eff.code()), label.clone());
        if let Some(v) = labels.get(&key) {
            return Some(v.clone());
        }
        let v = value_to_sample(gen_one(spec.name, rng, eff, &Args::new(p.args))?);
        labels.insert(key, v.clone());
        return Some(v);
    }
    Some(value_to_sample(gen_one(spec.name, rng, eff, &Args::new(p.args))?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(gen: &str, n: u32) -> GenRequest {
        GenRequest {
            generator: gen.into(),
            n,
            locale: None,
            seed: Some(42),
            args: None,
            template: None,
        }
    }

    fn gen(gen: &str, n: u32) -> GenResult {
        generate(&req(gen, n), Locale::En).unwrap()
    }

    fn strings(r: &GenResult) -> Vec<String> {
        r.values
            .iter()
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect()
    }

    #[test]
    fn every_generator_produces_non_empty_for_each_claimed_locale() {
        for spec in registry::CATALOG {
            for &loc in spec.supported {
                let mut r = req(spec.name, 3);
                r.locale = Some(loc.code().to_string());
                let out = generate(&r, Locale::En).unwrap();
                assert_eq!(out.values.len(), 3, "{}@{}", spec.name, loc.code());
                for v in &out.values {
                    let s = value_to_sample(v.clone());
                    assert!(!s.trim().is_empty(), "empty: {}@{}", spec.name, loc.code());
                }
                // A claimed locale must never report a fallback.
                assert!(!out.fell_back, "{}@{} wrongly fell back", spec.name, loc.code());
            }
        }
    }

    #[test]
    fn plausibility_of_key_generators() {
        let email = &strings(&gen("email", 1))[0];
        assert!(email.contains('@') && email.contains('.'), "email: {email}");
        let ip = &strings(&gen("ipv4", 1))[0];
        assert_eq!(ip.split('.').count(), 4, "ipv4: {ip}");
        let uuid = &strings(&gen("uuid", 1))[0];
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.matches('-').count(), 4, "uuid: {uuid}");
        let hexc = &strings(&gen("hex_color", 1))[0];
        assert!(hexc.starts_with('#') && hexc.len() == 7, "hex_color: {hexc}");
        let ean = &strings(&gen("ean", 1))[0];
        assert_eq!(ean.len(), 13);
        assert!(ean.chars().all(|c| c.is_ascii_digit()));
        assert!(iban_valid(&strings(&gen("iban", 1))[0]));
    }

    /// ISO 13616 mod-97: rearrange + convert + `% 97 == 1`.
    fn iban_valid(iban: &str) -> bool {
        let s: String = iban.chars().filter(|c| !c.is_whitespace()).collect();
        if s.len() < 5 {
            return false;
        }
        let rearranged = format!("{}{}", &s[4..], &s[..4]);
        let mut num = String::new();
        for c in rearranged.chars() {
            if c.is_ascii_digit() {
                num.push(c);
            } else if c.is_ascii_alphabetic() {
                num.push_str(&(c.to_ascii_uppercase() as u32 - 'A' as u32 + 10).to_string());
            } else {
                return false;
            }
        }
        let mut rem = 0u32;
        for c in num.chars() {
            rem = (rem * 10 + c.to_digit(10).unwrap()) % 97;
        }
        rem == 1
    }

    #[test]
    fn seed_is_deterministic() {
        let a = gen("name", 20);
        let b = gen("name", 20);
        assert_eq!(strings(&a), strings(&b));
        assert_eq!(a.seed, 42);
        // A different seed almost surely differs.
        let mut r = req("name", 20);
        r.seed = Some(43);
        let c = generate(&r, Locale::En).unwrap();
        assert_ne!(strings(&a), strings(&c));
    }

    #[test]
    fn random_seed_is_returned_and_reproducible() {
        let mut r = req("uuid", 5);
        r.seed = None;
        let first = generate(&r, Locale::En).unwrap();
        // Re-run pinned to the returned seed → identical.
        r.seed = Some(first.seed);
        let second = generate(&r, Locale::En).unwrap();
        assert_eq!(strings(&first), strings(&second));
    }

    #[test]
    fn locale_fallback_is_reported() {
        // Phone is not localised for German → EN + fell_back.
        let mut r = req("phone", 1);
        r.locale = Some("DE_DE".into());
        let out = generate(&r, Locale::DeDe).unwrap();
        assert_eq!(out.locale_used, "EN");
        assert!(out.fell_back);
        // Names ARE localised for German → used, no fallback.
        let mut r2 = req("first_name", 1);
        r2.locale = Some("DE_DE".into());
        let out2 = generate(&r2, Locale::DeDe).unwrap();
        assert_eq!(out2.locale_used, "DE_DE");
        assert!(!out2.fell_back);
    }

    #[test]
    fn n_bounds() {
        assert_eq!(gen("int", 1).values.len(), 1);
        assert_eq!(gen("int", 10_000).values.len(), 10_000);
        // n==0 clamps to 1.
        assert_eq!(gen("int", 0).values.len(), 1);
        // n>max errors.
        assert!(generate(&req("int", 10_001), Locale::En).is_err());
    }

    #[test]
    fn int_range_and_bool() {
        let mut r = req("int", 200);
        r.args = Some("1..6".into());
        let out = generate(&r, Locale::En).unwrap();
        for v in &out.values {
            let n = v.as_i64().unwrap();
            assert!((1..=6).contains(&n), "out of range: {n}");
        }
        let b = gen("bool", 10);
        assert!(b.values.iter().all(|v| v.is_boolean()));
    }

    #[test]
    fn composites_have_all_fields() {
        for name in ["person", "user", "address_full", "company_full", "order"] {
            let out = gen(name, 3);
            let fields = registry::lookup(name).unwrap().fields;
            for v in &out.values {
                let obj = v.as_object().unwrap();
                for f in fields {
                    assert!(obj.contains_key(*f), "{name} missing {f}");
                    assert!(!obj[*f].to_string().trim_matches('"').is_empty());
                }
            }
            let got: Vec<&str> = out.fields.as_ref().unwrap().iter().map(|s| s.as_str()).collect();
            assert_eq!(got.as_slice(), fields);
        }
    }

    #[test]
    fn catalog_covers_every_generator_with_a_sample() {
        let cat = catalog(Locale::DeDe);
        assert_eq!(cat.len(), registry::CATALOG.len());
        for e in &cat {
            assert!(!e.sample.is_empty(), "no sample for {}", e.name);
            assert!(!e.supported_locales.is_empty());
        }
    }

    #[test]
    fn template_renders_and_escapes() {
        let mut r = GenRequest {
            generator: String::new(),
            n: 1,
            locale: Some("EN".into()),
            seed: Some(7),
            args: None,
            template: Some("[{first_name}] {{literal}} {int:1..1}".into()),
        };
        let out = generate(&r, Locale::En).unwrap();
        let line = &strings(&out)[0];
        assert!(line.contains("{literal}"), "braces not literal: {line}");
        assert!(line.ends_with(" 1"), "int arg: {line}");
        assert!(line.starts_with('['));
        // Unknown placeholder stays verbatim.
        r.template = Some("{bogus_gen}".into());
        let out2 = generate(&r, Locale::En).unwrap();
        assert_eq!(strings(&out2)[0], "{bogus_gen}");
    }

    #[test]
    fn expand_faker_label_binding() {
        let mut ctx = FakerCtx::new(Locale::En);
        // Same label ⇒ same value.
        let a = expand_faker("first_name#k", &mut ctx).unwrap();
        let b = expand_faker("first_name#k", &mut ctx).unwrap();
        assert_eq!(a, b);
        // Different label ⇒ independent (may differ; assert the cache is keyed).
        let c = expand_faker("first_name#j", &mut ctx).unwrap();
        assert!(!c.is_empty());
        // Unknown generator ⇒ None (caller keeps it literal).
        assert!(expand_faker("bogus", &mut ctx).is_none());
    }

    #[test]
    fn expand_faker_is_freshly_seeded_per_context() {
        // Two independent contexts almost surely produce different uuids.
        let mut c1 = FakerCtx::new(Locale::En);
        let mut c2 = FakerCtx::new(Locale::En);
        assert_ne!(
            expand_faker("uuid", &mut c1).unwrap(),
            expand_faker("uuid", &mut c2).unwrap()
        );
    }

    #[test]
    fn unknown_generator_errors() {
        assert!(generate(&req("nope", 1), Locale::En).is_err());
    }

    #[test]
    fn registry_has_no_name_or_alias_collisions() {
        use std::collections::HashSet;
        let mut seen: HashSet<&str> = HashSet::new();
        let names: HashSet<&str> = registry::CATALOG.iter().map(|g| g.name).collect();
        for g in registry::CATALOG {
            assert!(seen.insert(g.name), "duplicate name {}", g.name);
            for a in g.aliases {
                assert!(seen.insert(a), "alias {a} collides");
                // An alias must never equal a DIFFERENT generator's name.
                assert!(!names.contains(a), "alias {a} shadows a generator name");
            }
        }
    }

    fn mem_db() -> crate::db::DbHandle {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let db = std::sync::Arc::new(parking_lot::Mutex::new(conn));
        crate::settings::init_table(&db).unwrap();
        db
    }

    #[test]
    fn defaults_round_trip_and_whitelist() {
        let db = mem_db();
        // Fresh DB → the built-in defaults (DE_DE, plain, count 1, history on).
        let d = get_defaults(&db).unwrap();
        assert_eq!(d.locale, "DE_DE");
        assert_eq!(d.format, "plain");
        assert!(d.save_history);

        set_defaults(
            &db,
            &FakerDefaults {
                locale: "FR_FR".into(),
                count: 25,
                format: "csv".into(),
                pinned: vec!["email".into(), "person".into(), "bogus".into()],
                save_history: false,
            },
        )
        .unwrap();
        let d2 = get_defaults(&db).unwrap();
        assert_eq!(d2.locale, "FR_FR");
        assert_eq!(d2.count, 25);
        assert_eq!(d2.format, "csv");
        assert!(!d2.save_history);
        // Unknown generator dropped from pinned; valid ones kept in order.
        assert_eq!(d2.pinned, vec!["email".to_string(), "person".to_string()]);

        // Garbage locale/format are rejected back to safe values.
        set_defaults(
            &db,
            &FakerDefaults { locale: "XX".into(), count: 0, format: "yaml".into(), pinned: vec![], save_history: true },
        )
        .unwrap();
        let d3 = get_defaults(&db).unwrap();
        assert_eq!(d3.locale, "DE_DE");
        assert_eq!(d3.format, "plain");
        assert_eq!(d3.count, 1);
    }

    #[test]
    fn every_catalog_name_is_generatable() {
        // Guards the metadata table against a name with no gen_one arm.
        for g in registry::CATALOG {
            let out = generate(&req(g.name, 1), Locale::En);
            assert!(out.is_ok(), "{} not generatable: {:?}", g.name, out.err());
        }
    }
}
