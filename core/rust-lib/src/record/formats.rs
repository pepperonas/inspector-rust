//! Several formats (16:9, 9:16 …) of ONE project. The document's root always holds the
//! active format; `EditDoc::variants` keeps what the other formats set differently: the
//! camera's look and layout and where the captions sit. Everything else — cuts, speed,
//! zooms, annotations, layout changes on the timeline, sound — is shared.
//!
//! A variant deliberately holds nothing that names a clip: no edit of the timeline can make
//! a stored format stale, and "edit, then switch" equals "switch, then edit" by construction.

use serde::{Deserialize, Serialize};

use super::editor::{Aspect, CameraStyle, CaptionAnchorH, CaptionAnchorV, CaptionSettings, EditDoc, LayoutPreset};

/// Where and how big the captions are in one format (the rest of their look is shared).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionPlace {
    pub anchor_v: CaptionAnchorV,
    pub anchor_h: CaptionAnchorH,
    pub inset_y: Option<f64>,
    pub inset_x: Option<f64>,
    pub font_size: f64,
}

/// What an inactive format keeps for itself. Its aspect is its identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatVariant {
    pub aspect: Aspect,
    pub camera: CameraStyle,
    pub captions: CaptionPlace,
}

/// The order formats are listed in (chips, batch export) — fixed, so switching never
/// reorders them.
pub const FORMAT_ORDER: [Aspect; 8] =
    [Aspect::Auto, Aspect::Source, Aspect::W16H9, Aspect::W9H16, Aspect::W1H1, Aspect::W4H3, Aspect::W3H4, Aspect::W21H9];
/// The formats that can be added (the two "as recorded" kinds exist only as a project's first).
pub const ADDABLE: [Aspect; 6] = [Aspect::W16H9, Aspect::W9H16, Aspect::W1H1, Aspect::W4H3, Aspect::W3H4, Aspect::W21H9];

fn rank(a: Aspect) -> usize {
    FORMAT_ORDER.iter().position(|x| *x == a).unwrap_or(FORMAT_ORDER.len())
}

pub fn place_of(c: &CaptionSettings) -> CaptionPlace {
    CaptionPlace { anchor_v: c.anchor_v, anchor_h: c.anchor_h, inset_y: c.inset_y, inset_x: c.inset_x, font_size: c.font_size }
}

fn with_place(c: &CaptionSettings, p: &CaptionPlace) -> CaptionSettings {
    CaptionSettings { anchor_v: p.anchor_v, anchor_h: p.anchor_h, inset_y: p.inset_y, inset_x: p.inset_x, font_size: p.font_size, ..c.clone() }
}

/// The active format's own part, as a variant.
fn active_variant(doc: &EditDoc) -> FormatVariant {
    FormatVariant { aspect: doc.output.aspect, camera: doc.camera.clone(), captions: place_of(&doc.captions) }
}

/// Every format of the project (the active one included), in [`FORMAT_ORDER`].
pub fn formats(doc: &EditDoc) -> Vec<Aspect> {
    let mut all: Vec<Aspect> = std::iter::once(doc.output.aspect).chain(doc.variants.iter().map(|v| v.aspect)).collect();
    all.sort_by_key(|a| rank(*a));
    all.dedup();
    all
}

/// One variant per aspect (the first wins), none for the active aspect, in [`FORMAT_ORDER`];
/// each one's camera and caption place through the same clamps as the root's.
pub fn normalize_variants(
    variants: Vec<FormatVariant>,
    active: Aspect,
    captions: &CaptionSettings,
    camera: impl Fn(CameraStyle) -> CameraStyle,
) -> Vec<FormatVariant> {
    let mut out: Vec<FormatVariant> = Vec::new();
    for v in variants {
        if v.aspect == active || out.iter().any(|o| o.aspect == v.aspect) {
            continue;
        }
        let place = place_of(&super::captions::normalize_settings(with_place(captions, &v.captions)));
        out.push(FormatVariant { aspect: v.aspect, camera: camera(v.camera), captions: place });
    }
    out.sort_by_key(|v| rank(v.aspect));
    out
}

/// The document with `aspect` as the active format: its stored look moves to the root, the
/// root's look is stored in its place. `None` when the project has no such format; the
/// active one returns the document unchanged.
pub fn switch_variant(doc: &EditDoc, aspect: Aspect) -> Option<EditDoc> {
    if doc.output.aspect == aspect {
        return Some(doc.clone());
    }
    let i = doc.variants.iter().position(|v| v.aspect == aspect)?;
    let mut next = doc.clone();
    let incoming = next.variants.remove(i);
    next.variants.push(active_variant(doc));
    next.variants.sort_by_key(|v| rank(v.aspect));
    next.output.aspect = incoming.aspect;
    next.camera = incoming.camera;
    next.captions = with_place(&doc.captions, &incoming.captions);
    Some(next)
}

/// Taller than wide.
pub fn is_portrait(a: Aspect) -> bool {
    a.ratio().is_some_and(|r| r < 1.0)
}

/// Adds `aspect` as a new format and makes it the active one; an existing one is just
/// switched to. The new format starts with the current look. Two things are reset because
/// they are positions in the OLD frame: a freely placed camera goes back to its corner and
/// the captions to the new aspect's standard distance. A portrait format of a project with
/// a camera starts stacked (screen above, camera below) instead of picture-in-picture.
pub fn add_variant(doc: &EditDoc, aspect: Aspect, has_camera: bool) -> EditDoc {
    if let Some(d) = switch_variant(doc, aspect) {
        return d;
    }
    let mut next = doc.clone();
    next.variants.push(active_variant(doc));
    next.variants.sort_by_key(|v| rank(v.aspect));
    next.output.aspect = aspect;
    next.camera.position = None;
    if has_camera && is_portrait(aspect) && doc.camera.preset == LayoutPreset::PictureInPicture {
        next.camera.preset = LayoutPreset::VerticalStack;
    }
    next.captions.inset_y = None;
    next.captions.inset_x = None;
    next
}

/// Removes a format. Removing the active one switches to the first other format; the last
/// format of a project cannot be removed (the document comes back unchanged).
pub fn remove_variant(doc: &EditDoc, aspect: Aspect) -> EditDoc {
    if doc.output.aspect != aspect {
        let mut next = doc.clone();
        next.variants.retain(|v| v.aspect != aspect);
        return next;
    }
    let Some(first) = doc.variants.first().map(|v| v.aspect) else { return doc.clone() };
    let mut next = switch_variant(doc, first).unwrap_or_else(|| doc.clone());
    next.variants.retain(|v| v.aspect != aspect);
    next
}

/// File-name part of a format (`9x16`); empty for the two "as recorded" kinds.
pub fn file_suffix(a: Aspect) -> &'static str {
    match a {
        Aspect::Auto | Aspect::Source => "",
        Aspect::W16H9 => "16x9",
        Aspect::W9H16 => "9x16",
        Aspect::W1H1 => "1x1",
        Aspect::W4H3 => "4x3",
        Aspect::W3H4 => "3x4",
        Aspect::W21H9 => "21x9",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::{normalize, CameraPosition, EditClip, TakeMetas};

    fn doc() -> EditDoc {
        let mut d: EditDoc = serde_json::from_value(serde_json::json!({
            "version": 8,
            "clips": [{"startMs": 0, "endMs": 10_000}],
            "output": {"aspect": "16:9"},
        }))
        .unwrap();
        d.camera.size_pct = 30.0;
        d.camera.position = Some(CameraPosition { x: 0.8, y: 0.2 });
        d.captions.enabled = true;
        d.captions.font_size = 40.0;
        d.captions.inset_y = Some(7.0);
        d
    }

    #[test]
    fn a_new_portrait_format_starts_stacked_with_fresh_positions() {
        let d = doc();
        let p = add_variant(&d, Aspect::W9H16, true);
        assert_eq!(p.output.aspect, Aspect::W9H16);
        assert_eq!(p.camera.preset, LayoutPreset::VerticalStack);
        assert_eq!((p.camera.position, p.captions.inset_y), (None, None));
        assert_eq!((p.camera.size_pct, p.captions.font_size, p.captions.enabled), (30.0, 40.0, true), "the look carries over");
        // The old format is kept exactly as it was.
        assert_eq!(p.variants, vec![active_variant(&d)]);
        assert_eq!(formats(&p), vec![Aspect::W16H9, Aspect::W9H16]);
        // Without a camera, or for a landscape format, the layout stays.
        assert_eq!(add_variant(&d, Aspect::W9H16, false).camera.preset, LayoutPreset::PictureInPicture);
        assert_eq!(add_variant(&d, Aspect::W1H1, true).camera.preset, LayoutPreset::PictureInPicture);
    }

    #[test]
    fn switching_there_and_back_changes_nothing() {
        let d = doc();
        let mut p = add_variant(&d, Aspect::W9H16, true);
        p.camera.size_pct = 45.0;
        p.captions.anchor_v = CaptionAnchorV::Top;
        let back = switch_variant(&p, Aspect::W16H9).unwrap();
        assert_eq!((back.output.aspect, &back.camera, &back.captions), (d.output.aspect, &d.camera, &d.captions));
        let again = switch_variant(&back, Aspect::W9H16).unwrap();
        assert_eq!(again, p);
        assert_eq!(switch_variant(&d, Aspect::W16H9), Some(d.clone()), "the active format: unchanged");
        assert_eq!(switch_variant(&d, Aspect::W1H1), None, "no such format");
        // Adding a format that exists only switches to it.
        assert_eq!(add_variant(&back, Aspect::W9H16, true), p);
    }

    #[test]
    fn shared_things_stay_shared_across_formats() {
        let d = doc();
        let mut p = add_variant(&d, Aspect::W9H16, true);
        // An edit made in the portrait format: a cut and another caption colour …
        p.clips = vec![EditClip::main(0, 4_000), EditClip::main(6_000, 10_000)];
        p.crop = vec![None, None];
        p.captions.color = "#ffcc00".into();
        p.mic_gain_db = -3.0;
        let land = switch_variant(&p, Aspect::W16H9).unwrap();
        // … is there in the landscape format too, which keeps its own caption place.
        assert_eq!((land.clips.len(), land.captions.color.as_str(), land.mic_gain_db), (2, "#ffcc00", -3.0));
        assert_eq!(land.captions.inset_y, Some(7.0));
    }

    /// The order of "edit the timeline" and "switch the format" does not matter: a variant
    /// holds nothing the timeline could invalidate.
    #[test]
    fn edit_then_switch_equals_switch_then_edit() {
        let takes = TakeMetas::new();
        let base = add_variant(&doc(), Aspect::W9H16, true);
        type Edit = Box<dyn Fn(&mut EditDoc)>;
        let edits: Vec<Edit> = vec![
            Box::new(|d| d.clips = vec![EditClip::main(0, 3_000), EditClip::main(3_000, 10_000)]),
            Box::new(|d| d.clips = vec![EditClip::main(2_000, 9_000)]),
            Box::new(|d| d.clips = vec![EditClip::main(5_000, 10_000), EditClip::main(0, 5_000)]),
            Box::new(|d| d.clips.clear()),
        ];
        for (i, edit) in edits.iter().enumerate() {
            let mut a = base.clone();
            edit(&mut a);
            let a = switch_variant(&normalize(a, 10_000, &takes), Aspect::W16H9).unwrap();
            let mut b = switch_variant(&base, Aspect::W16H9).unwrap();
            edit(&mut b);
            let b = normalize(b, 10_000, &takes);
            assert_eq!(normalize(a, 10_000, &takes), b, "edit {i}");
        }
    }

    #[test]
    fn removing_formats() {
        let d = doc();
        let p = add_variant(&add_variant(&d, Aspect::W9H16, true), Aspect::W1H1, true);
        assert_eq!(formats(&p), vec![Aspect::W16H9, Aspect::W9H16, Aspect::W1H1]);
        // An inactive one just goes.
        assert_eq!(formats(&remove_variant(&p, Aspect::W9H16)), vec![Aspect::W16H9, Aspect::W1H1]);
        // The active one: the first other format takes over, with ITS look.
        let r = remove_variant(&p, Aspect::W1H1);
        assert_eq!((r.output.aspect, formats(&r)), (Aspect::W16H9, vec![Aspect::W16H9, Aspect::W9H16]));
        assert_eq!(r.camera, d.camera);
        // The last format stays.
        assert_eq!(remove_variant(&d, Aspect::W16H9), d);
    }

    #[test]
    fn stored_formats_are_cleaned_up() {
        let mut d = add_variant(&doc(), Aspect::W9H16, true);
        let mut dup = d.variants[0].clone();
        dup.camera.size_pct = 999.0;
        dup.captions.font_size = f64::NAN;
        // A second entry for the same aspect, one for the active aspect, one out of order.
        let mut active_twin = dup.clone();
        active_twin.aspect = Aspect::W9H16;
        let mut square = dup.clone();
        square.aspect = Aspect::W1H1;
        d.variants = vec![square, d.variants[0].clone(), dup, active_twin];
        let n = normalize(d, 10_000, &TakeMetas::new());
        assert_eq!(n.variants.iter().map(|v| v.aspect).collect::<Vec<_>>(), vec![Aspect::W16H9, Aspect::W1H1]);
        assert_eq!(n.variants[0].camera.size_pct, 30.0, "the first entry for an aspect wins");
        let sq = &n.variants[1];
        assert!(sq.camera.size_pct <= 60.0 && sq.captions.font_size.is_finite(), "clamped like the root");
    }

    #[test]
    fn every_addable_format_has_a_file_name_and_a_place_in_the_order() {
        for a in ADDABLE {
            assert!(!file_suffix(a).is_empty() && FORMAT_ORDER.contains(&a), "{a:?}");
        }
        assert_eq!((file_suffix(Aspect::Auto), file_suffix(Aspect::W9H16)), ("", "9x16"));
        assert!(is_portrait(Aspect::W9H16) && is_portrait(Aspect::W3H4) && !is_portrait(Aspect::W1H1) && !is_portrait(Aspect::Auto));
    }
}
