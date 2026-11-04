//! Built-in shader effects (stage 7): our own catalogue of six WGSL effects for the compositor's
//! shader-effect pass (contract v1, `vendor/openscreen-compositor/src/shader_fx.rs`), the effect
//! regions of the editor document, their normalisation and their `shaderEffects` scene entries.
//!
//! The parameters are declared ONCE, in `assets/record/effects/params.json`: this registry reads
//! it, the record-render compile test reads it, and so does the UI. The WGSL code is embedded
//! here; record-render compiles every file through the compositor's own validation
//! (`every_builtin_effect_compiles`). Upstream ships no built-ins, and users cannot load WGSL:
//! the catalogue is the only source of effect code.

use super::editor::{EditClip, MIN_CLIP_MS};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

/// The contract version every built-in is written against.
pub const CONTRACT: u32 = 1;
/// The compositor's ceiling on declared parameters (`PARAMS_BUFFER_BYTES` / 16).
pub const MAX_PARAMS: usize = 32;

/// The one declaration of every built-in's parameters (also read by record-render and the UI).
pub const PARAMS_JSON: &str = include_str!("../../assets/record/effects/params.json");

/// The code of each built-in, by id.
const CODE: &[(&str, &str)] = &[
    ("vignette", include_str!("../../assets/record/effects/vignette.wgsl")),
    ("grain", include_str!("../../assets/record/effects/grain.wgsl")),
    ("chromatic", include_str!("../../assets/record/effects/chromatic.wgsl")),
    ("ripple", include_str!("../../assets/record/effects/ripple.wgsl")),
    ("spotlight", include_str!("../../assets/record/effects/spotlight.wgsl")),
    ("scanlines", include_str!("../../assets/record/effects/scanlines.wgsl")),
];

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum EffectParamKind {
    Number { min: f64, max: f64, default: f64 },
    Toggle { default: bool },
    /// `#rrggbb` or `#rrggbbaa`.
    Color { default: String },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EffectParam {
    pub name: String,
    #[serde(flatten)]
    pub kind: EffectParamKind,
}

impl EffectParam {
    /// The compositor's parameter type name.
    pub fn type_name(&self) -> &'static str {
        match self.kind {
            EffectParamKind::Number { .. } => "number",
            EffectParamKind::Toggle { .. } => "toggle",
            EffectParamKind::Color { .. } => "color",
        }
    }

    pub fn default_value(&self) -> EffectValue {
        match &self.kind {
            EffectParamKind::Number { default, .. } => EffectValue::Number(*default),
            EffectParamKind::Toggle { default } => EffectValue::Toggle(*default),
            EffectParamKind::Color { default } => EffectValue::Color(default.clone()),
        }
    }

    /// `v` made valid for this parameter: numbers clamped into the range, a wrong type, a
    /// non-finite number or an unreadable colour falls back to the default.
    pub fn normalize_value(&self, v: EffectValue) -> EffectValue {
        match (&self.kind, v) {
            (EffectParamKind::Number { min, max, default }, EffectValue::Number(n)) => {
                EffectValue::Number(if n.is_finite() { n.clamp(*min, *max) } else { *default })
            }
            (EffectParamKind::Toggle { .. }, EffectValue::Toggle(b)) => EffectValue::Toggle(b),
            (EffectParamKind::Color { .. }, EffectValue::Color(c)) if parse_hex_color(&c).is_some() => {
                EffectValue::Color(c.trim().to_ascii_lowercase())
            }
            _ => self.default_value(),
        }
    }
}

/// One built-in effect.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinEffect {
    pub id: String,
    pub code: &'static str,
    pub params: Vec<EffectParam>,
    /// Reads `ctx.cursor` / `ctx.clicks`: meaningless without pointer data.
    pub needs_pointer: bool,
}

impl BuiltinEffect {
    pub fn param(&self, name: &str) -> Option<&EffectParam> {
        self.params.iter().find(|p| p.name == name)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Declared {
    id: String,
    needs_pointer: bool,
    params: Vec<EffectParam>,
}

/// The catalogue, in its display order (`params.json`).
pub fn builtin_effects() -> &'static [BuiltinEffect] {
    static ALL: OnceLock<Vec<BuiltinEffect>> = OnceLock::new();
    ALL.get_or_init(|| {
        // A bundled file: a parse error is a build mistake the tests catch, never user input.
        let declared: Vec<Declared> = serde_json::from_str(PARAMS_JSON).expect("assets/record/effects/params.json");
        declared
            .into_iter()
            .filter_map(|d| {
                let code = CODE.iter().find(|(id, _)| *id == d.id)?.1;
                Some(BuiltinEffect { id: d.id, code, params: d.params, needs_pointer: d.needs_pointer })
            })
            .collect()
    })
}

pub fn builtin(id: &str) -> Option<&'static BuiltinEffect> {
    builtin_effects().iter().find(|e| e.id == id)
}

/// A parameter's value as the document stores it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EffectValue {
    Number(f64),
    Toggle(bool),
    Color(String),
}

impl EffectValue {
    /// The compositor's value: a number, a toggle as 0/1, a colour as 4 floats 0..1.
    fn scene_json(&self) -> Value {
        match self {
            EffectValue::Number(n) => json!(n),
            EffectValue::Toggle(b) => json!(if *b { 1.0 } else { 0.0 }),
            EffectValue::Color(c) => json!(parse_hex_color(c).unwrap_or([0.0, 0.0, 0.0, 1.0])),
        }
    }
}

/// `#rrggbb` / `#rrggbbaa` → RGBA 0..1.
pub fn parse_hex_color(c: &str) -> Option<[f64; 4]> {
    let hex = c.trim().strip_prefix('#')?;
    if !matches!(hex.len(), 6 | 8) || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|b| b as f64 / 255.0);
    Some([byte(0)?, byte(2)?, byte(4)?, if hex.len() == 8 { byte(6)? } else { 1.0 }])
}

/// One shader effect on the programme timeline. Times are SOURCE ms of its clip (like
/// [`super::editor::ZoomRegion`]); `values` holds only what differs from the effect's defaults
/// as the user set them (every parameter is filled in when the scene is built).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectRegion {
    #[serde(default)]
    pub id: String,
    pub clip: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    /// A built-in's id (`builtin`).
    pub effect: String,
    #[serde(default)]
    pub values: BTreeMap<String, EffectValue>,
}

/// One effect region on the programme timeline. `index` points into `EditDoc::effects`; effects
/// never overlap, so `row` is always 0 (the lane has one row).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectSpan {
    pub index: usize,
    pub id: String,
    pub effect: String,
    pub programme_start_ms: u64,
    pub programme_end_ms: u64,
    pub row: u32,
}

/// Re-index onto the surviving clips (a deleted clip takes its effects along), clamp into the
/// clip, drop unknown effects, keep only declared parameters (values made valid), resolve
/// overlaps — one effect at a time, the later in the list wins and cuts the earlier — then drop
/// slivers under `MIN_CLIP_MS` and make ids unique. Sorted by (clip, start).
pub fn normalize_effects(
    regions: Vec<EffectRegion>,
    clips: &[EditClip],
    new_index: impl Fn(usize) -> Option<usize>,
) -> Vec<EffectRegion> {
    let mut resolved: Vec<EffectRegion> = Vec::new();
    for r in regions {
        let Some(def) = builtin(&r.effect) else { continue };
        let Some(clip) = new_index(r.clip) else { continue };
        let c = &clips[clip];
        let start = r.start_ms.clamp(c.start_ms, c.end_ms);
        let end = r.end_ms.clamp(c.start_ms, c.end_ms);
        if end <= start {
            continue;
        }
        let values: BTreeMap<String, EffectValue> = r
            .values
            .into_iter()
            .filter_map(|(name, v)| def.param(&name).map(|p| (name, p.normalize_value(v))))
            .collect();
        let new = EffectRegion { clip, start_ms: start, end_ms: end, values, ..r };
        let mut next = Vec::with_capacity(resolved.len() + 1);
        for e in resolved {
            if e.clip != clip || e.end_ms <= start || e.start_ms >= end {
                next.push(e);
                continue;
            }
            // The later region cuts its range out of the earlier one (head and tail may stay).
            if e.start_ms < start {
                next.push(EffectRegion { end_ms: start, ..e.clone() });
            }
            if e.end_ms > end {
                next.push(EffectRegion { start_ms: end, ..e });
            }
        }
        next.push(new);
        resolved = next;
    }
    resolved.retain(|r| r.end_ms - r.start_ms >= MIN_CLIP_MS);
    resolved.sort_by_key(|r| (r.clip, r.start_ms));
    let mut taken = HashSet::new();
    for r in &mut resolved {
        r.id = unique_id(std::mem::take(&mut r.id), &mut taken);
    }
    resolved
}

fn unique_id(id: String, taken: &mut HashSet<String>) -> String {
    if !id.trim().is_empty() && taken.insert(id.clone()) {
        return id;
    }
    let mut n = taken.len() + 1;
    loop {
        let candidate = format!("effect-{n}");
        if taken.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

/// The region's `shaderEffects` entry (`SceneShaderEffect`, `V/src/scene.rs:688-718`): source
/// seconds of scene clip `clip_index`, the whole pill in programme seconds (`ctx.region`),
/// target image, contract 1, the code, the declared parameters in `FxParams` order and every
/// value filled in (colour → 4 floats, toggle → 0/1). `None` for an unknown effect.
pub fn scene_entry(r: &EffectRegion, clip_index: usize, programme_ms: (u64, u64)) -> Option<Value> {
    let def = builtin(&r.effect)?;
    let params: Vec<Value> = def.params.iter().map(|p| json!({"name": p.name, "type": p.type_name()})).collect();
    let values: serde_json::Map<String, Value> = def
        .params
        .iter()
        .map(|p| {
            let v = r.values.get(&p.name).cloned().map_or_else(|| p.default_value(), |v| p.normalize_value(v));
            (p.name.clone(), v.scene_json())
        })
        .collect();
    let sec = |ms: u64| ms as f64 / 1000.0;
    Some(json!({
        "clipIndex": clip_index,
        "startSec": sec(r.start_ms),
        "endSec": sec(r.end_ms),
        "programmeStartSec": sec(programme_ms.0),
        "programmeEndSec": sec(programme_ms.1),
        "target": "image",
        "contract": CONTRACT,
        "code": def.code,
        "params": params,
        "values": Value::Object(values),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(start: u64, end: u64) -> EditClip {
        EditClip { start_ms: start, end_ms: end, source: None }
    }

    fn region(clip: usize, start: u64, end: u64, effect: &str) -> EffectRegion {
        EffectRegion { id: String::new(), clip, start_ms: start, end_ms: end, effect: effect.into(), values: BTreeMap::new() }
    }

    #[test]
    fn catalogue_ids_are_unique_and_known() {
        let ids: Vec<&str> = builtin_effects().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["vignette", "grain", "chromatic", "ripple", "spotlight", "scanlines"]);
        for e in builtin_effects() {
            assert!(!e.params.is_empty() && e.params.len() <= MAX_PARAMS, "{}", e.id);
            let names: HashSet<&str> = e.params.iter().map(|p| p.name.as_str()).collect();
            assert_eq!(names.len(), e.params.len(), "{}: duplicate parameter", e.id);
            for p in &e.params {
                // The compositor's name rule (`check_params`): snake_case, a-z first, ≤ 32.
                assert!(
                    p.name.len() <= 32
                        && p.name.starts_with(|c: char| c.is_ascii_lowercase())
                        && p.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                        && p.name != "unused",
                    "{}.{}",
                    e.id,
                    p.name
                );
                match &p.kind {
                    EffectParamKind::Number { min, max, default } => {
                        assert!(min < max && (min..=max).contains(&default), "{}.{}", e.id, p.name)
                    }
                    EffectParamKind::Color { default } => assert!(parse_hex_color(default).is_some()),
                    EffectParamKind::Toggle { .. } => {}
                }
                assert!(e.code.contains(&format!("params.{}", p.name)), "{}.{} is never read", e.id, p.name);
            }
            assert!(e.code.contains("fn effect(uv: vec2<f32>) -> vec4<f32>"), "{}", e.id);
            let pointer = e.code.contains("ctx.cursor") || e.code.contains("ctx.clicks");
            assert_eq!(pointer, e.needs_pointer, "{}: needs_pointer", e.id);
        }
        assert!(builtin("ripple").unwrap().code.contains("ctx.clicks"));
        assert!(builtin("spotlight").unwrap().code.contains("ctx.cursor"));
        assert!(builtin("nope").is_none());
        // Every WGSL file is in the declaration and every declaration has its file.
        assert_eq!(CODE.len(), builtin_effects().len());
    }

    /// The one declaration record-render's compile test reads: `[{id, needsPointer, params:
    /// [{name, type, …}]}]` whose `{name, type}` pairs are exactly the registry's.
    #[test]
    fn catalogue_params_json() {
        let v: Value = serde_json::from_str(PARAMS_JSON).unwrap();
        let from_file: Vec<(String, Vec<(String, String)>)> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let params = e["params"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| (p["name"].as_str().unwrap().to_string(), p["type"].as_str().unwrap().to_string()))
                    .collect();
                (e["id"].as_str().unwrap().to_string(), params)
            })
            .collect();
        let from_registry: Vec<(String, Vec<(String, String)>)> = builtin_effects()
            .iter()
            .map(|e| (e.id.clone(), e.params.iter().map(|p| (p.name.clone(), p.type_name().to_string())).collect()))
            .collect();
        assert_eq!(from_file, from_registry);
    }

    #[test]
    fn every_wgsl_stays_small_and_resolution_independent() {
        for e in builtin_effects() {
            let lines = e.code.lines().count();
            assert!(lines <= 60, "{}: {lines} lines", e.id);
            assert!(!e.code.contains("@group") && !e.code.contains("@binding"), "{}", e.id);
            // Raw pixel sizes would differ between the small preview and the export.
            assert!(!e.code.contains("ctx.resolution"), "{}: pixel units, use fx_px", e.id);
        }
    }

    #[test]
    fn effects_without_overlap() {
        let clips = [clip(0, 10_000)];
        let id = |i: usize| Some(i);
        let n = normalize_effects(
            vec![region(0, 1_000, 5_000, "vignette"), region(0, 3_000, 7_000, "grain")],
            &clips,
            id,
        );
        let spans: Vec<_> = n.iter().map(|r| (r.start_ms, r.end_ms, r.effect.as_str())).collect();
        assert_eq!(spans, vec![(1_000, 3_000, "vignette"), (3_000, 7_000, "grain")]);
        // The cut-down earlier one goes when under MIN_CLIP_MS.
        let n = normalize_effects(
            vec![region(0, 1_000, 5_000, "vignette"), region(0, 1_050, 7_000, "grain")],
            &clips,
            id,
        );
        assert_eq!(n.iter().map(|r| r.effect.as_str()).collect::<Vec<_>>(), ["grain"]);
        // Regions on different clips never collide; ids come out unique.
        let n = normalize_effects(
            vec![region(0, 1_000, 5_000, "vignette"), region(1, 12_000, 14_000, "grain")],
            &[clip(0, 10_000), clip(10_000, 20_000)],
            id,
        );
        assert_eq!(n.len(), 2);
        assert_ne!(n[0].id, n[1].id);
    }

    #[test]
    fn unknown_effect_or_param_is_dropped_and_values_clamped() {
        let clips = [clip(0, 10_000)];
        let mut r = region(0, 1_000, 5_000, "vignette");
        r.values.insert("strength".into(), EffectValue::Number(7.0));
        r.values.insert("radius".into(), EffectValue::Toggle(true));
        r.values.insert("softness".into(), EffectValue::Number(f64::NAN));
        r.values.insert("color".into(), EffectValue::Color("red".into()));
        r.values.insert("bogus".into(), EffectValue::Number(1.0));
        let n = normalize_effects(vec![r, region(0, 6_000, 8_000, "deep-fry")], &clips, Some);
        assert_eq!(n.len(), 1, "an unknown effect is dropped");
        let v = &n[0].values;
        assert_eq!(v.get("strength"), Some(&EffectValue::Number(1.0)), "above max → max");
        assert_eq!(v.get("radius"), Some(&EffectValue::Number(0.75)), "wrong type → default");
        assert_eq!(v.get("softness"), Some(&EffectValue::Number(0.45)), "NaN → default");
        assert_eq!(v.get("color"), Some(&EffectValue::Color("#000000".into())), "bad colour → default");
        assert!(!v.contains_key("bogus"), "an undeclared value is removed");
        // A good colour is kept (normalised spelling).
        let mut r = region(0, 0, 2_000, "vignette");
        r.values.insert("color".into(), EffectValue::Color(" #FF8800CC ".into()));
        assert_eq!(normalize_effects(vec![r], &clips, Some)[0].values["color"], EffectValue::Color("#ff8800cc".into()));
        // Clamped into the clip; a region outside it goes.
        let n = normalize_effects(vec![region(0, 8_000, 30_000, "grain"), region(0, 12_000, 13_000, "grain")], &clips, Some);
        assert_eq!(n.iter().map(|r| (r.start_ms, r.end_ms)).collect::<Vec<_>>(), [(8_000, 10_000)]);
    }

    #[test]
    fn scene_entry_fills_every_value() {
        let mut r = region(0, 1_000, 3_000, "grain");
        r.values.insert("animated".into(), EffectValue::Toggle(false));
        let e = scene_entry(&r, 2, (5_000, 7_000)).unwrap();
        assert_eq!(e["clipIndex"], 2);
        assert_eq!(e["values"]["animated"], 0.0);
        assert_eq!(e["values"]["amount"], 0.15);
        assert_eq!(e["values"]["size"], 1.5);
        assert!(scene_entry(&region(0, 0, 1, "nope"), 0, (0, 1)).is_none());
        assert_eq!(parse_hex_color("#ff000080"), Some([1.0, 0.0, 0.0, 128.0 / 255.0]));
        assert_eq!(parse_hex_color("#fff"), None);
    }
}
