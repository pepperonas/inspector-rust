# `record` Etappe 3 — Zoom, Zeiger, Bild — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Der `record`-Editor macht aus einer Rohaufnahme ein Demo-Video im Screen-Studio-Stil: automatische und manuelle Zooms, gerenderter Mauszeiger (Themes, Glättung, Klick-Bounce, Klick-Impact, Klick-Ton, Bewegungsunschärfe, Auto-Hide), Hintergrund (eigene Wallpaper, Verläufe, Farbe, eigenes Bild, Bewegung), Padding, Eckenradius, Schatten, Hintergrund-Blur, Bewegungsunschärfe und Gerätrahmen (Fenster, Laptop, Handy, Monitor; hell/dunkel) — in Vorschau und Export gleich.

**Architecture:** Gezeichnet wird alles vom vendorten Compositor aus `Scene`-Feldern (`zoomRegions`, `cursor`, `effects`, `background`, `clickSound`, `layout.screenRect`). Portiert werden nur die TS-Datenschritte von OpenScreen nach reinem Rust in `core/rust-lib/src/record/`: Auto-Zoom-Planer, Klick-Erkennung und Klick-Ton-Platzierung, Layout-Rechteck mit Rahmen-Platz und Auto-Seitenverhältnis. Das Editor-Dokument wächst auf `version: 2` (Migration von v1 mit unverändertem Aussehen); der Render-Helfer reicht die Effektwerte zusätzlich als LiveView-Parameter durch, damit die Vorschau dem Export gleicht.

**Tech Stack:** Rust (core, `record-render`, vendorter Compositor), React 19 + TS, Vitest, `image`-Crate für die Wallpaper-Erzeugung.

**Spec:** `docs/superpowers/specs/2026-11-01-record-design.md` (Etappe 3; Abschnitte Zoom, Zeiger, Bild; Anti-Ziele). Recon dieser Etappe: in Task 1 nach `vendor/openscreen-compositor/RECON-stage3.md` kopieren (Quelle: Scratchpad `stage3-recon.md` dieser Sitzung). Etappe-2-Recon: `vendor/openscreen-compositor/RECON.md`. Referenzquelle (nur lesen, nie ausführen): Klon `openscreen-src/new` (Pfad in RECON.md). Alle Upstream-Zitate unten in der Form `C/…:Zeile` (TS-Klon) bzw. `V/…:Zeile` (vendorter Compositor) beziehen sich auf RECON-stage3.md.

## Global Constraints

- Nur macOS Apple Silicon (wie Etappe 2); Linux/Windows/Intel bauen unverändert grün.
- Verhalten 1:1 wie OpenScreen v2.1.2 (`bfd5bc32`): gleiche Konstanten, gleiche Planerregeln; Bewegungs-Mathematik (Federkurve, Log-Skala, Verkettung, Totzone, Zeiger-Feder, Bounce, Pinning, Impact) **nicht** neu schreiben — sie liegt im Compositor und bleibt dort.
- Zoom-Tiefen **1→1,25 · 2→1,5 · 3→1,8 · 4→2,2 · 5→3,5 · 6→5,0×**, Standard-Tiefe 3, eigene Skala 1,0–5,0; neue manuelle Zoomregion: 2 s, Tiefe 3, Fokus (0,5; 0,5), `focusMode` auto.
- Neue Dokumente (Upstream-Standards, `C/src/lib/projectDefaults.ts:236-285`): Padding 50 % [0,100], Eckenradius 40 px [0,64], Schatten 0,6 [0,1], Hintergrund-Blur 0 [0,1], Bewegungsunschärfe 0,2 [0,1], Rahmen `none`, Rahmen-Theme `light`, Seitenverhältnis `auto`; Zeiger: sichtbar, Auto-Hide aus, Theme `default`, Größe 3,6 [1,5;6], Glättung 0,3 [0,1], Bewegungsunschärfe 0,35 [0,1], Klick-Bounce 1 [0,2], Klick-Impact aus, Klick-Ton aus, Klick-Ton-Pegel 0 dB [−24,0]. Standard-Hintergrund: unser erstes eigenes Wallpaper.
- **Keine** der 18 OpenScreen-Wallpaper (`C/public/wallpapers/`, ohne Lizenz) — weder Datei noch Ableitung. Eigene Wallpaper prozedural erzeugt, Generator im Repo. Zeiger-PNGs (OpenScreen-Eigenbau, MIT) und Kenney-Klicks (CC0) dürfen übernommen werden, mit Hinweis in den Third-Party-Notices.
- Stage-7-Felder bleiben aus: `rotation: null`, `model3d: false`, kein `sculpt`, `depthOfField: false`.
- `EDITOR_VERSION` 1 → **2**; ein v1-Dokument öffnet mit **unverändertem Aussehen** (schwarz, kein Padding, kein Zeiger, keine Zooms, Seitenverhältnis wie gespeichert). `PROJECT_SCHEMA` bleibt 1.
- Eigene Bilder/Zeiger werden ins Projekt kopiert (`<projekt>/assets/`), damit ein Projekt beim Verschieben vollständig bleibt; fehlt eine referenzierte Datei, fällt der Szenenbau auf den Standard zurück — nie ein Helfer-Fehler.
- Alle UI-Texte en+de; `.claude/rules/animation.md`; kein neues npm-Paket; keine neue Runtime-Crate außer was schon im Workspace liegt.
- Laufwerk knapp: `CARGO_INCREMENTAL=0`, `df -h /System/Volumes/Data` vor schweren Builds (Abbruch unter 1,5 GB). Der Compositor braucht `MAC_FFMPEG_DIR=third_party/ffmpeg/8.1.2`.

## Review Focus

1. **Aufnahme ohne Zeigerdaten** (Accessibility fehlte, `provider: "none"`, leere `samples`) → Auto-Zoom-Knopf aus mit Hinweis, Zeiger-Bereich sagt „keine Zeigerdaten“, Szene `cursor.show=false`, Zooms mit `focusMode: auto` bleiben auf ihrem Fokus. (Task 4 `no_cursor_data_disables_cursor`, Task 3 `no_samples_plans_nothing`.)
2. **Schnitt durch eine Zoomregion / Clip gelöscht / Trim verkürzt** → Zoom wird mitgeschnitten bzw. entfernt, nie außerhalb seines Clips, nie kürzer als 0,25 s. (Task 2 `normalize_zoom_follows_split`, `normalize_drops_zoom_of_deleted_clip`.)
3. **Etappe-2-Projekt (Editor-Dokument v1)** → öffnet ohne Look-Änderung. (Task 2 `v1_doc_migrates_without_visual_change`.)
4. **Eigenes Hintergrundbild oder eigener Zeiger nachträglich gelöscht** → Szene fällt auf Standard zurück, Editor zeigt Hinweis. (Task 4 `missing_custom_image_falls_back`, `missing_custom_cursor_falls_back`.)
5. **Vorschau ≠ Export mit Effekten** (Schatten, Radius, Padding, Zeigergröße) → gleich bis auf Encoder-Rauschen. (Task 5 `preview_matches_still_with_effects`.)

---

### Task 1: Assets, Wallpaper-Generator, Compositor-Patch für den Zeiger-Pfad

**Files:**
- Create: `core/rust-lib/assets/record/cursors/<theme>/*.png` (aus `C/public/cursors/**`, 7 Themes), `core/rust-lib/assets/record/sounds/{click-down,click-up}.wav` (aus `C/public/sounds/`), `core/rust-lib/assets/record/cursors/themes.json`, `core/rust-lib/assets/record/wallpapers/{01..08}.jpg` + `thumbs/{01..08}.jpg`, `core/rust-lib/src/record/assets.rs`, `vendor/openscreen-compositor/RECON-stage3.md`
- Modify: `vendor/openscreen-compositor/src/{scene.rs,timeline_walk.rs,live.rs}`, `vendor/openscreen-compositor/VENDORED.md`, `vendor/openscreen-compositor/THIRD-PARTY-NOTICES.md` (Abschnitt „Inspector Rust assets“), `scripts/bundle-record-render.sh` (Assets nach `Contents/Resources/record/`), `core/rust-lib/src/record/mod.rs`

**Interfaces (Produces):**
- `SceneClip.cursor_path: Option<String>` (serde `cursorPath`, Standard `None` = altes Verhalten `{screenPath}.cursor.json`); Export-Walk (`timeline_walk.rs:498`) und LiveView (`live.rs:179,1527,1751` sowie `LiveView::create`) laden daraus. Patch 12 in VENDORED.md.
- `record::assets::AssetRoot { dir: PathBuf }` mit `AssetRoot::resolve(app: &AppHandle) -> AssetRoot` (Bundle: `resource_dir()/record`; Dev/Test: `CARGO_MANIFEST_DIR/assets/record`), `fn wallpaper(&self, id: &str) -> Option<PathBuf>`, `fn wallpaper_ids() -> &'static [&'static str]` (`"01".."08"`), `fn cursor_theme(&self, theme: &str) -> Option<CursorTheme>`, `fn click_sounds(&self) -> (PathBuf, PathBuf)`; `CursorTheme { id: String, sprites: Vec<(String /*state*/, PathBuf, f32 /*hotspot x frac*/, f32 /*y*/)> }` gebaut wie upstream `resolveCursorSprites` (Basis `default`, Theme legt `arrow`/`pointer` darüber, Hotspot = Upstream-Wert / Bildbreite bzw. -höhe; `asArrow`-Gruppe `["text","grab","resize","busy","crosshair","not-allowed","help","up-arrow"]` nimmt das Pfeil-Sprite, Gruppierung nach `CURSOR_KINDS`, `C/src/lib/cursor/cursorThemes.ts:105-145`).
- `themes.json`: Theme-IDs `default, studio-ink, prism-glow, pop-coral, pixel-candy, star-sprout, glass-lens` mit Hotspots (aus `cursorThemes.ts` abgeschrieben, Default-Hotspots aus `DEFAULT_CURSOR_SPRITES :73-97`), damit Rust keine TS-Datei liest.

- [ ] **Step 1:** RECON-stage3.md ablegen. Dateien aus dem Klon kopieren (nur `cp` von PNG/WAV, vorher `file` je Datei: muss „PNG image data“ bzw. „RIFF … WAVE“ sagen, sonst nicht übernehmen). Notices: Zeiger = OpenScreen Original Artwork (MIT, Zitat aus `cursorThemes.ts`), Klicks = Kenney (CC0, `C/THIRD-PARTY-NOTICES.md:253-263`).
- [ ] **Step 2: Wallpaper-Generator** als `#[ignore]`-Test `gen_record_wallpapers` in `assets.rs` (läuft nur auf Zuruf, schreibt nach `CARGO_MANIFEST_DIR/assets/record/wallpapers`): 8 deterministische Motive (feste Seeds; weiche Mesh-Verläufe aus 3–5 Farbflecken mit Gauß-Abfall + feines Rauschen gegen Banding), 2560×1440 JPEG q85, Thumbs 320×180. Paletten frei, aber deutlich unterscheidbar (hell/dunkel gemischt; mindestens 3 dunkle, 3 helle). Generator laufen lassen, Ergebnis committen; Summe < 4 MB.
- [ ] **Step 3: Failing tests** in `assets.rs`: `every_theme_resolves_all_16_states` (jeder Zustand aus `contracts.ts:8-24` hat Pfad + Hotspot in [0,1], Datei existiert) · `as_arrow_states_use_arrow_sprite` · `wallpaper_ids_all_exist` · `click_sounds_exist`. Vendor: `cursor_path_overrides_sidecar_location` (scene.rs-Test: JSON mit `cursorPath` → Feld gesetzt; ohne → `None`).
- [ ] **Step 4:** Implementieren, Patch 12 eintragen; `bundle-record-render.sh` kopiert `assets/record` ohne `thumbs/`-Ausnahme nach `Contents/Resources/record/` (Thumbs braucht die Oberfläche, mitkopieren).
- [ ] **Step 5:** `cargo test -p inspector-rust-core --lib record::assets` und `MAC_FFMPEG_DIR=third_party/ffmpeg/8.1.2 CARGO_INCREMENTAL=0 cargo test -p openscreen-compositor --lib scene` grün. Commit `feat(record): cursor themes, click sounds, own wallpapers, cursor sidecar path`.

---

### Task 2: Editor-Dokument v2 (Aussehen, Zeiger, Zooms) + Migration

**Files:**
- Modify: `core/rust-lib/src/record/editor.rs`, `core/frontend/src/lib/ipc.ts` (Typen `RecordEditDoc` …), `core/frontend/src/lib/editor.ts` (reine Operationen)
- Test: `editor.rs`-Testmodul, `core/frontend/src/lib/editor.test.ts`

**Interfaces (Produces)** — alles `serde(rename_all="camelCase")`, TS spiegelt 1:1:
- `EDITOR_VERSION: u32 = 2`.
- `EditDoc` erhält `#[serde(default)] appearance: Appearance`, `#[serde(default)] cursor: CursorStyle`, `#[serde(default)] zooms: Vec<ZoomRegion>`, `#[serde(default)] auto_zoom_done: bool` (Merker: automatische Erstplanung lief schon; v1-Migration setzt `true`, damit alte Projekte nicht nachträglich Zooms bekommen).
- `Appearance { background: Background, background_motion: WallpaperMotion, padding: f64 /*0..100*/, radius_px: f64 /*0..64*/, shadow: f64, background_blur: f64, motion_blur: f64, frame: Frame, frame_theme: FrameTheme }`; `Appearance::default()` = Upstream-Standards (Global Constraints) mit `Background::Wallpaper { id: "01" }`; `Appearance::plain()` = `Background::Color{color:"#000000"}`, alle Zahlen 0, `Frame::None`.
- `Background` (`#[serde(tag="kind", rename_all="lowercase")]`): `Wallpaper { id: String }` · `Gradient { angle_deg: f64, stops: Vec<String> }` · `Color { color: String }` · `Image { file: String /*relativ zu <projekt>/assets*/ }`.
- `WallpaperMotion` lowercase `none|drift|aurora|waves`; `Frame` kebab `none|window|laptop|phone|monitor`; `FrameTheme` lowercase `light|dark`.
- `CursorStyle { show: bool, auto_hide: bool, theme: String, custom: Option<CustomCursor>, size: f64, smoothing: f64, motion_blur: f64, click_bounce: f64, click_impact: bool, click_sound: bool, click_sound_gain_db: f64 }`; `CustomCursor { file: String, hotspot_x: f64, hotspot_y: f64 }`; `CursorStyle::default()` = Upstream-Standards, `CursorStyle::plain()` = `show: false`, sonst Standard.
- `ZoomRegion { id: String, clip: usize, start_ms: u64, end_ms: u64 /*SOURCE ms des Clips, wie SpeedRegion*/, depth: u8 /*1..6*/, custom_scale: Option<f64>, focus_x: f64, focus_y: f64, focus_mode: FocusMode /*manual|auto*/, origin: ZoomOrigin /*manual|auto*/, hide_cursor: bool }`.
- `pub const ZOOM_DEPTHS: [f64; 6] = [1.25, 1.5, 1.8, 2.2, 3.5, 5.0]`, `DEFAULT_ZOOM_DEPTH: u8 = 3`, `MIN_ZOOM_MS: u64 = 250`, `NEW_ZOOM_MS: u64 = 2000`; `fn zoom_scale(z: &ZoomRegion) -> f64` (custom_scale geklemmt [1,5], sonst Tabelle).
- `Aspect` erhält `#[serde(rename="auto")] Auto` (Standard für neue Dokumente; `ratio()` → `None` wie Source, die echte Rechnung macht Task 4).
- `fn migrate(value: serde_json::Value) -> Option<EditDoc>`: v1 → v2 mit `Appearance::plain()`, `CursorStyle::plain()`, `zooms: []`, Aspect unverändert; v2 direkt; sonst `None`. `load()` nutzt `migrate`.
- `normalize` zusätzlich: Zooms auf ihren Clip klemmen; nach `splitAt` gehört eine Zoomregion zu dem Teil, der ihren Startpunkt enthält, und wird an der Schnittkante gekürzt; Regionen < `MIN_ZOOM_MS` und Regionen gelöschter Clips fallen weg; `clip`-Indizes werden beim Löschen/Teilen nachgezogen (wie `speed`); Überlappung im selben Clip → spätere Region verkürzt die frühere; Zahlen auf ihre Bereiche geklemmt; Tiefe auf 1..6.
- TS (`lib/editor.ts`): `addZoom(doc, clip, sourceMs) -> doc` (2 s ab sourceMs, im Clip geklemmt, Tiefe 3, Fokus Mitte, auto, origin manual, id `crypto.randomUUID()`), `updateZoom(doc, id, patch)`, `removeZoom(doc, id)`, `removeAutoZooms(doc)`, `setAppearance(doc, patch)`, `setCursorStyle(doc, patch)`; `ZOOM_DEPTHS` gespiegelt.

- [ ] **Step 1: Failing tests (Rust):** `v1_doc_migrates_without_visual_change` (v1-JSON aus dem Etappe-2-Golden → `appearance == plain`, `cursor.show == false`, `zooms` leer, Aspect wie gespeichert) · `new_doc_gets_upstream_defaults` (padding 50, radius 40, shadow 0.6, motion_blur 0.2, cursor size 3.6, smoothing 0.3, aspect Auto) · `unknown_version_falls_back_to_default` · `zoom_scale_table` (Tiefe 1..6 → Tabelle; custom 7 → 5; custom 0.5 → 1) · `normalize_zoom_follows_split` (Zoom 1000–5000 in Clip 0, Split bei 3000 → Zoom 1000–3000 in Clip 0, nichts in Clip 1) · `normalize_drops_zoom_of_deleted_clip` · `normalize_drops_short_zoom` (200 ms) · `normalize_clamps_appearance` (padding 150 → 100, radius −3 → 0) · `serde_roundtrip_v2`.
- [ ] **Step 2: Failing tests (TS):** `addZoom` clampt am Clip-Ende · `removeAutoZooms` lässt manuelle stehen · Typ-Spiegel: `ZOOM_DEPTHS` gleich Rust (Test liest `editor.rs` per `fs` von Platte und vergleicht die Konstante — wie die bestehenden Rust-Pins).
- [ ] **Step 3:** Implementieren. Alle bestehenden Tests, die `EDITOR_VERSION`-Literale oder das Golden nutzen, auf v2 bringen, ohne ihre Aussage zu ändern.
- [ ] **Step 4:** `cargo test -p inspector-rust-core --lib record::editor` + `pnpm --filter inspector-rust-frontend exec vitest run src/lib/editor.test.ts` grün. Commit `feat(record): editor document v2 with appearance, cursor style and zooms`.

---

### Task 3: Auto-Zoom-Planer (Port von `zoom-suggestions.ts`)

**Files:**
- Create: `core/rust-lib/src/record/zoom_plan.rs`
- Modify: `core/rust-lib/src/record/mod.rs`

**Interfaces:**
- Consumes: `EditDoc`, `ZoomRegion`, `ZOOM_DEPTHS`, `zoom_scale` (Task 2); `CursorRecording`/`CursorSample` aus `record/project.rs`.
- Produces: `pub fn detect_clicks(samples: &[CursorSample], total_ms: u64) -> Vec<Click>` mit `Click { time_ms: u64, cx: f64, cy: f64 }`; `pub fn plan_auto_zooms(doc: &EditDoc, cursor: &CursorRecording) -> Vec<ZoomRegion>` (neue Regionen mit `origin: Auto, focus_mode: Auto`, ids `auto-<clip>-<start_ms>`; bestehende `doc.zooms` sind reserviert und werden nie verändert); `pub fn zoom_transition_ms(scale: f64) -> f64 = 600 + 550·ln(max(1,scale))`.

Algorithmus und **alle** Konstanten 1:1 aus RECON-stage3 §2a–2c (`OPENING_WIDE_MS=2500`, `CLOSING_WIDE_MS=1000`, `STOP_CLICK_GUARD_MS=1000`, `SHOT_MERGE_GAP_MS=3000`, `SHOT_LEAD_MS=500`, `SHOT_TAIL_MS=1500`, `MIN_SHOT_HOLD_MS=1800`, `MIN_WIDE_BETWEEN_MS=1500`, `SHOT_FIT_MARGIN=0.12`, Planer-Tiefen `[1,2,3]`, `UNRELEASED_PRESS_GRACE_MS=150`, reservierte Regionen ohne Skala zählen 1,8). Upstream-Quellen zum Mitlesen: `C/src/lib/ai-edition/timeline/zoom-suggestions.ts` + `.test.ts`, `apply-auto-zooms.ts`.

**Zeitachsen-Entscheidung:** Upstream plant pro Clip auf Quellzeit und prüft Abstände in *Timeline*-ms ohne Tempo. Bei uns: Edit-Zeit = Summe der Quell-Dauern der vorigen Clips (Tempo ignoriert, wie upstream). `wideUntil`/`wideFrom` je Clip = `2500 − editOffset(i)` bzw. `editEnd − 1000 − editOffset(i)`; Stop-Klick-Grenze = letzter Sample-Zeitpunkt − 1000; Clip-Fenster = `[start_ms, end_ms]` der Quelle, Samples in Clip-lokale Zeit verschoben; danach ein globales `resolveShots` über alle Clips auf Edit-Zeit; Ergebnis zurück in Quell-ms je Clip.

- [ ] **Step 1: Failing tests** — die Upstream-Fälle aus `zoom-suggestions.test.ts` 1:1 mit denselben Zahlen (Liste in RECON-stage3 §2e, Zeilen `:30-490`). Mindestens: Klick-Auswahl (`hidden_click_ignored`, `press_hidden_during_drag_ignored`, `release_before_later_hide_kept`, `legacy_click_without_mouseup_kept_when_hide_late`, `…_rejected_when_hide_next_tick`, `all_four_click_kinds_accepted`, `move_and_mouseup_are_not_clicks`, `parked_pointer_no_zoom`, `no_samples_plans_nothing`), Shots (`single_click_at_2000_in_5000` → 1500–3500 Tiefe 3 Fokus = Klick; `burst_2000_3500_5000` → 1500–6500 Tiefe 3 Fokus (0,35; 0,325); `spread_035_depth_2`, `spread_05_depth_1`; `clash_keeps_more_clicks` → 1500–4100; `gap_4s_same_spot_merges` → 1500–7500; `wide_shot_kept` → 1500–3500 und 8500–10500; `reserved_3000_5000_blocks_7000_allows_9000`; `reserved_5x_ramps`; `axis_ends` → 0–1800 und 3200–5000), Clips (`click_per_clip`, `source_in_point_shift`, `outside_window_ignored`, `reserve_per_clip`, `clip_without_samples_skipped`, `no_hold_across_cut`, `opening_and_ending_wide`, `stop_click_ignored`). Zusätzlich: `existing_zooms_never_change`, `double_click_counts_once_as_shot_click`, `position_only_provider_accepted`.
- [ ] **Step 2:** Run → FAIL (Modul fehlt).
- [ ] **Step 3:** Implementieren (Rechenreihenfolge, `round`/`floor`/`ceil` exakt wie upstream).
- [ ] **Step 4:** `cargo test -p inspector-rust-core --lib record::zoom_plan` grün; drei Mutationen (Merge-Abstand 3000→3001, Fit-Rand weg, Reserviert-gewinnt umgedreht) müssen je mindestens einen Test rot machen — Ergebnis in den Bericht.
- [ ] **Step 5:** Commit `feat(record): auto-zoom planner ported from OpenScreen`.

---

### Task 4: Layout, Auto-Seitenverhältnis, Klick-Ton-Platzierung, Szenenbau

**Files:**
- Create: `core/rust-lib/src/record/layout.rs`, `core/rust-lib/src/record/click_cues.rs`
- Modify: `core/rust-lib/src/record/scene_json.rs`, `core/rust-lib/src/record/editor.rs` (`output_size` für `Aspect::Auto`), `core/rust-lib/src/record/editor_ipc.rs` (AssetRoot + Projektordner an den Szenenbau), `core/rust-lib/src/record/fixtures/scene-golden.json` (+ neues `scene-golden-styled.json`)

**Interfaces:**
- Consumes: Task 1 `AssetRoot`, Task 2 Dokument, Task 3 nichts.
- Produces:
  - `layout.rs` (Port des **No-Webcam-Zweigs** von `C/src/lib/compositeLayout.ts` + `C/src/lib/frameFootprint.ts`, Konstanten aus `V/src/frame_geometry.rs:485-492,637-757`): `pub fn padding_fit(padding_pct: f64) -> f64 = 1 − padding/100·0.4`; `pub fn screen_rect(out: (u32,u32), screen_aspect: f64, padding_pct: f64, frame: Frame) -> Rect` (normiert 0..1, enthält Rahmen-Platz); `pub fn auto_aspect(screen_aspect: f64, padding_pct: f64, frame: Frame) -> f64` (`autoFormatAspect`, mit Rahmen 8 Fixpunkt-Durchläufe, `C/src/lib/compositeLayout.ts:432-445`); `pub fn max_zoom_scale(screen_rect: Rect, out: (u32,u32), src: (u32,u32)) -> f64` (`maxZoomScaleFor`, `MAX_SOURCE_MAGNIFICATION=2`, Referenz 1080, `C/src/lib/ai-edition/timeline/zoom-scale.ts:89-93`).
  - `click_cues.rs` (Port `buildClickCues` + `clickPlacement`, `C/src/lib/ai-edition/clickSound.ts:23-99,252ff`): `pub fn click_hits(doc: &EditDoc, cursor: &CursorRecording) -> Vec<ClickHit>` mit `ClickHit { prog_ms: u64, gain: f64, release: bool }` — Programmzeit **inkl. Tempo** (über `source_to_programme`); Konstanten `DOUBLE_TAP_MS=90`, `MAX_RELEASE_GAP_MS=600`, `FALLBACK_RELEASE_MS=110`, Seitenklick-Pegel 0,85, Release ×0,7; Klicks in herausgeschnittenen Bereichen fallen weg.
  - `scene_json(project_dir, project, doc, out, fps, assets: &AssetRoot) -> Value`: füllt `effects` (padding/100, blur, shadow, `roundnessFrac = (padding>0 || frame≠none) ? radius_px/1080 : 0`, motionBlur, `frame`/`frameTheme` nur wenn Rahmen an bzw. dunkel, `depthOfField:false`), `background` (Wallpaper → `image{path: absolute}`; Gradient → `gradient{angleDeg, stops}`; Color; Image → `<projekt>/assets/<file>`, fehlt → Standard-Wallpaper; `motion` nur bei Gradient/Image/Wallpaper und ≠ none), `layout.screenRect` aus `layout::screen_rect`, `zoomRegions` (je Zoom: `clipIndex`, Quell-Sekunden, `scale = min(zoom_scale, max_zoom_scale)`, `focusX/Y`, `focusMode`, `rotation: null`, `hideCursor` nur wenn wahr), `cursor` (Felder RECON-stage3 §1d; `cursorSprites` aus `AssetRoot::cursor_theme` bzw. eigenem Zeiger für `arrow` + asArrow-Gruppe; `model3d:false`, `clipToBounds:false`), `clickSound` (nur wenn `cursor.show && click_sound` und Treffer existieren; `gain` × `10^(gainDb/20)`), jede Clip-Zeile `cursorPath` = `<projekt>/cursor.json`. Ohne Zeigerdaten (`provider` none oder keine Samples): `cursor.show=false`, kein `clickSound`.
  - `output_size(meta, aspect, target)`: `Auto` → `auto_aspect(...)` mit Lang-Seite aus dem Ziel (720/1080/Quelle), gerade Pixel (`toEvenPx = max(2, round(v/2)·2)`).

- [ ] **Step 1: Failing tests:** `padding_fit_values` (0 → 1, 50 → 0,8, 100 → 0,6) · `screen_rect_centred_without_frame` · `frame_reserves_room` (Fenster-Rahmen: Rechteck kleiner als ohne, Titelleiste oben `WINDOW_FRAME_BAR_FRAC=0.04`) · `auto_aspect_without_padding_is_screen_aspect` · `auto_aspect_with_padding_flattens_elongation` (`e' = fit·e + (1−fit)`) · `max_zoom_scale_limits_small_sources` · `click_hits_project_through_speed` (Klick bei Quelle 3,5 s, Tempo ×2 auf 1–3 s → Programm 2,5 s) · `double_click_adds_second_press_at_90ms` · `release_fallback_110ms` · `cut_out_click_dropped` · `no_cursor_data_disables_cursor` · `missing_custom_image_falls_back` · `missing_custom_cursor_falls_back` · `styled_scene_matches_golden` (neues Golden mit allen Effekten, Pfade durch Platzhalter ersetzt wie im bestehenden Golden-Test) · bestehender `scene_matches_golden` unverändert grün für ein v1-migriertes Dokument (Look-Gleichheit belegt). Zusätzlich ein Parse-Test im Compositor-Crate-Stil wie in Etappe 2: das neue Golden muss `Scene::from_json` überstehen (in `record-render` als Test, das den Compositor kennt).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implementieren; `editor_ipc` reicht `AssetRoot::resolve(app)` und den Projektordner durch (einmal pro Sitzung auflösen).
- [ ] **Step 4:** Tests grün (`record::layout`, `record::click_cues`, `record::scene_json`, `cargo test -p record-render scene`). Commit `feat(record): layout, click cues and styled scene`.

---

### Task 5: Render-Helfer — Vorschau gleich Export

**Files:**
- Modify: `record-render/src/session.rs`, `record-render/tests/helper_live.rs`

**Interfaces:**
- Consumes: Szenen-JSON aus Task 4; Patch 12 aus Task 1.
- Produces: `fn live_params(scene: &serde_json::Value) -> LiveParamSet` mit `LiveParamSet { nums: Vec<(&'static str, f64)>, bools: Vec<(&'static str, bool)>, strs: Vec<(&'static str, String)> }` — Schlüssel exakt wie `V/src/live.rs:1133-1174`: `shadow`, `roundness`, `motionBlur`, `padding`, `backgroundBlur`, `cursorSize`, `cursorClickBounce`, `cursorSmoothing`, `cursorMotionBlur`, `cursorShow`, `cursorAutoHide`, `backgroundColor` (nur bei `kind: color`). Einheiten so, dass die LiveView dieselben `LiveParams` erhält wie der Export über `frame_geometry::live_params_from_scene` (`V/src/frame_geometry.rs:1856-1875`) — insbesondere `roundness` aus `roundnessFrac` umrechnen; den Faktor aus `live_params_from_scene` ablesen, nicht raten. Nach **jedem** `set_scene` (Open, SetScene, Neustart) werden alle Parameter gesetzt. `LiveView::create` bekommt den `cursorPath` des ersten Clips statt `""`.

- [ ] **Step 1: Failing tests:** `live_params_map_every_effect` (Szene mit allen Werten ≠ Standard → jeder Schlüssel mit erwartetem Wert) · `live_params_match_export_params` (für drei Szenen: aus `live_params` gebaute `InspectorParams`-Wirkung == `live_params_from_scene` — vergleiche die resultierenden `LiveParams`-Felder; wenn der Compositor dafür keinen öffentlichen Weg hat, die Abbildung `InspectorParams → LiveParams` aus `live.rs:1797-1811` als Testhilfe nachbauen und das in VENDORED.md nicht patchen) · `params_reapplied_after_set_scene` (Fake-LiveView-Trait oder Aufrufprotokoll).
- [ ] **Step 2:** Run → FAIL. Implementieren.
- [ ] **Step 3: Live-Parität** (`#[ignore]`, braucht `IR_RECORD_PROJECT`): `preview_matches_still_with_effects` — Projekt mit gesetztem Padding 50, Radius 40, Schatten 0,6, Wallpaper 01, Zeiger an, ein Zoom; Vorschaubild bei t = 2,0 s gegen `Thumbs`-Standbild gleicher Größe: mittlere absolute Pixelabweichung < 3 (0–255). Laufen lassen; Messwert in den Bericht.
- [ ] **Step 4:** `cargo test -p record-render` grün. Commit `fix(record): preview takes the same effect values as export`.

---

### Task 6: Editor — Bild- und Zeiger-Einstellungen

**Files:**
- Create: `core/frontend/src/components/editor/Inspector.tsx` (Reiter **Hintergrund · Rahmen · Zeiger**), `core/frontend/src/components/editor/Inspector.test.tsx`
- Modify: `core/frontend/src/components/RecordEditor.tsx` (Inspector rechts neben der Vorschau, einklappbar), `core/rust-lib/src/record/editor_ipc.rs` (+ `lib.rs`-Registrierung), `core/frontend/src/lib/ipc.ts`, `core/frontend/src/locales/{en,de}.json`, `core/frontend/src/mockup/backend.ts`

**Interfaces:**
- Consumes: Task 2 TS-Operationen, `editorUpdate(sessionId, doc)` (bestehend).
- Produces IPC (async + `spawn_blocking`): `editor_assets() -> EditorAssets { wallpapers: Vec<{id, thumb: String /*asset-URL-fähiger Pfad*/}>, themes: Vec<{id, preview: String}>, has_cursor_data: bool }`; `editor_import_asset(session_id: u64, src: String, kind: "image"|"cursor") -> String` (kopiert nach `<projekt>/assets/<uuid>.<ext>`, nur png/jpg/jpeg, ≤ 25 MB, gibt den Dateinamen zurück; Bilder über Asset-Protokoll anzeigen — Scope für den Projektordner in `tauri.conf.json` aller drei Plattformen prüfen/ergänzen).

UI-Inhalt (Werte und Bereiche aus Global Constraints): **Hintergrund:** Wallpaper-Raster (8 Thumbs), Verläufe (die 16 Upstream-Paare `linear-gradient(135deg, a, b)` aus RECON-stage3 §4 als `Gradient{angle_deg:135, stops:[a,b]}`), Farbpalette (16 Werte §4), eigenes Bild (Dateidialog → `editor_import_asset`), Bewegung none/drift/aurora/waves (nur bei Wallpaper/Verlauf/Bild), Hintergrund-Blur. **Rahmen:** Padding, Eckenradius, Schatten (Stufen 0/0,3/0,6/0,9 als Chips + Regler), Bewegungsunschärfe, Gerätrahmen none/window/laptop/phone/monitor, Theme hell/dunkel, Seitenverhältnis (bestehende Presets + Auto). **Zeiger:** anzeigen, Auto-Hide, Theme-Raster (7, Vorschau = `arrow.png`), eigener Zeiger (PNG + Hotspot-X/Y-Regler), Größe, Glättung, Bewegungsunschärfe, Klick-Bounce (Stufen 0/1/2), Klick-Impact, Klick-Ton + Pegel. Ohne Zeigerdaten: Zeiger-Reiter zeigt „Diese Aufnahme hat keine Zeigerdaten“ und keine Regler. Regler senden entprellt (150 ms, wie der Mikro-Pegel) über `editorUpdate`; jede Änderung ist ein Undo-Schritt beim Loslassen, nicht pro Pixel.

- [ ] **Step 1: Failing tests** (`Inspector.test.tsx`, IPC gemockt): Wallpaper-Klick setzt `background` · Bewegung deaktiviert bei Farbe · Regler klemmt auf Bereich · ohne Zeigerdaten keine Zeiger-Regler · Undo-Schritt nur beim Loslassen · jeder Text existiert in en und de (bestehender i18n-Test deckt Schlüssel ab).
- [ ] **Step 2:** Implementieren. Rust-Test für `editor_import_asset`: `import_rejects_non_image`, `import_copies_into_project_assets`.
- [ ] **Step 3:** Sichtprüfung im Mockup (`core/frontend/mockup.html`, erfundene Daten) bei 1280×800: Inspector überdeckt die Vorschau nicht, alle drei Reiter erreichbar; Screenshot in den Bericht. `pnpm check` (bzw. `bash scripts/check.sh`) + `pnpm test` grün; App-Chunk-Budget (`scripts/check-bundle.mjs`) eingehalten — der Editor ist ein eigenes Fenster, Inspector darf nicht in den Popup-Chunk geraten.
- [ ] **Step 4:** Commit `feat(record): background, frame and cursor settings in the editor`.

---

### Task 7: Editor — Zoom-Spur, Fokus, Auto-Zoom, Klick-Ton in der Vorschau, Doku, Release

**Files:**
- Modify: `core/frontend/src/components/editor/Timeline.tsx` (Zoom-Spur), `core/frontend/src/components/editor/Inspector.tsx` (Reiter **Zoom** für die gewählte Region), `core/frontend/src/components/editor/Preview.tsx` (Fokus-Fadenkreuz beim Bearbeiten), `core/frontend/src/components/editor/AudioPreview.tsx` (Klick-Ton), `core/frontend/src/components/RecordEditor.tsx` (Taste Z, Auto-Zoom-Knopf), `core/rust-lib/src/record/editor_ipc.rs`, `core/rust-lib/src/record/editor.rs` (`EditorView.click_hits`), `core/rust-lib/src/record/session.rs` (Auto-Zoom beim ersten Öffnen), Locales, `CHANGELOG.md`, `features.txt`, `CLAUDE.md` (Abschnitt „record — stage 3“), `core/frontend/src/lib/commandDocs.ts` (record-Doc ergänzen), README via `node scripts/gen-docs.mjs`, Versionen (Etappe 3 = nächste Minor)
- Test: `Timeline.test.tsx` (neu), `editor_ipc.rs`-Tests

**Interfaces:**
- Consumes: Task 3 `plan_auto_zooms`, Task 4 `click_hits`, Task 2 TS-Operationen.
- Produces: IPC `editor_auto_zoom(session_id) -> EditorView` (plant mit `plan_auto_zooms`, hängt an, normalisiert, speichert; ein Undo-Schritt im Frontend); `EditorView.click_hits: Vec<ClickHit>`; Einstellung `record.auto_zoom` (Standard `true`): beim **ersten** Anlegen eines v2-Dokuments mit Zeigerdaten wird einmal automatisch geplant.

Verhalten: Zoom-Spur unter den Clips, Blöcke in Programmzeit (Umrechnung nur über die vorhandenen Rust-IPCs `editor_to_programme`/`editor_to_source` — keine TS-Kopie der Abbildung, Regel R12), Auto-Zooms sichtbar anders markiert als manuelle; ziehen = verschieben, Ränder = trimmen, Entf = löschen, **Z** = neue Region am Abspielkopf. Reiter Zoom: Tiefe 1–6 als Chips mit Faktor (1,25× …), eigene Skala, Fokus „folgt dem Zeiger“ (auto) / „fest“ (manual), bei fest: Klick in die Vorschau setzt den Fokus (Fadenkreuz-Overlay), Zeiger beim Zoom ausblenden. Knöpfe „Auto-Zoom“ (aus ohne Zeigerdaten, mit Tooltip) und „Auto-Zooms entfernen“. Klick-Ton-Vorschau: `AudioPreview` spielt `click-down`/`click-up` zu `click_hits` relativ zur Wiedergabeuhr, nur bei laufender Wiedergabe und eingeschaltetem Klick-Ton, Pegel wie im Export.

- [ ] **Step 1: Failing tests:** `Timeline.test.tsx`: Z legt Region am Abspielkopf an · Entf löscht gewählte · Auto- und manuelle Region unterscheidbar (Klasse/Attribut) · Ziehen über Clipgrenze klemmt. Rust: `auto_zoom_appends_and_keeps_manual` · `auto_zoom_without_cursor_data_is_noop` · `first_open_plans_once` (zweites Öffnen plant nicht erneut, auch wenn alle Zooms gelöscht wurden — Merker `auto_zoom_done` aus Task 2) · `click_hits_in_view`.
- [ ] **Step 2:** Implementieren.
- [ ] **Step 3:** Doku-Vertrag (CLAUDE.md „Documentation contract“): `features.txt`-Zeilen (Auto-Zoom, Zeiger-Stil, Hintergründe/Rahmen), CHANGELOG englisch, CommandDoc `record` erweitert, `node scripts/gen-docs.mjs`, Locales en+de. `bash scripts/check.sh` + `pnpm test` grün.
- [ ] **Step 4: Ende-zu-Ende** mit echter Aufnahme (Display wach halten: `caffeinate -u -t 1`): kurze Aufnahme mit drei Klicks → Editor öffnet mit Auto-Zooms; Wallpaper, Padding 50, Laptop-Rahmen, Zeiger-Theme `pop-coral`, Klick-Ton an; MP4 und GIF exportieren; Standbild aus dem MP4 (ffmpeg) zeigt Rahmen, Zeiger, Zoom; Zahlen (Dauer, Größe) in den Bericht. Danach Testaufnahme löschen (enthält Raumton).
- [ ] **Step 5:** `scripts/install-macos.sh`, Bundle prüft `Contents/Resources/record/{cursors,sounds,wallpapers}`; Commit `feat(record): zoom track, auto-zoom, click sound preview (vX.Y.0)`.
