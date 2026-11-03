# `record` Etappe 5 — Webcam mit Layouts und Freistellung — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `record` nimmt optional die Webcam als eigene Datei auf (30 fps, nativ), und der Editor setzt sie in den fünf OpenScreen-Layouts ins Bild (Bild-im-Bild, nebeneinander, übereinander, nur Kamera, ohne) — mit Anker, Größe, Maske, Rundung, Spiegeln, Ausschnitt, Schrumpfen beim Zoom, Layout-Wechseln innerhalb des Videos und Hintergrund-Freistellung (transparent, Weichzeichnen, eigener Hintergrund). Vorschau und Export sind gleich.

**Architecture:** Die Aufnahme bekommt einen dritten Schreiber neben Bildschirm und Mikrofon: `AVCaptureSession` → `AVCaptureVideoDataOutput` → eigener `AVAssetWriter` (`camera.mp4`, H.264), auf derselben Pausenuhr und demselben Ursprung wie der Bildschirm, damit der Kamera-Versatz 0 ist. Gezeichnet wird alles vom vendorten Compositor (Spiegeln, Maske, Schatten, Schrumpfen, Layout-Morphs, Freistellung im Shader); die **Layout-Rechtecke rechnet aber die App** (Upstream: TS `computeCompositeLayout`) — das wird in `record/layout.rs` portiert und als `webcamRect`/`screenRect`/`layoutByClip`/`layoutRegions` in die Szene geschrieben. Die Freistellung läuft im Render-Helfer auf **unserem** `ort` 2.0.0-rc.12 (statisch), Vendor-Patch 16.

**Tech Stack:** Rust (core: `objc2-av-foundation`, Compositor-Helfer `record-render`, vendorter Compositor), React 19 + TS, Vitest.

**Spec:** `docs/superpowers/specs/2026-11-01-record-design.md` (Etappe 5, Abschnitt „Webcam“). Recon mit Zeilenbelegen: `vendor/openscreen-compositor/RECON-stage5.md` — `C/` = Upstream-Klon im Scratchpad, nur lesen.

## Global Constraints

- Nur macOS Apple Silicon (wie Etappen 2–4); Linux/Windows/Intel bauen unverändert grün. Windows-Cross-Check ohne neue Diagnosen.
- Verhalten 1:1 wie OpenScreen v2.1.2 (`bfd5bc32`), Konstanten aus `C/src/lib/compositeLayout.ts` und `C/src/lib/projectDefaults.ts` (Belege in RECON §2b): `MARGIN_FRACTION 0.02` von `min(W,H)`; `BLOCK_GAP_FRACTION 0.02` der Bildschirmbreite; `BLOCK_CAMERA_ASPECT_TOLERANCE 1.25`; Radius-Regel nebeneinander `{max 24, min 12, frac 0.06}`, übereinander `{max 24, min 8, frac 0.06}`; `LAYOUT_TRANSITION_SECONDS 0.2`; Größe 15–60 %, Standard 40; Maske `square|rectangle`, Rundung Standard `square 0.7`, `rectangle 0.4`; Spiegeln aus; Schrumpfen beim Zoom an (nur Bild-im-Bild); Anker `bottom-right`; Freistellung Standard `none`, Weichzeichnen 0.5. Schatten, Schrumpf-Faktor (0.7) und Morphs rechnet der Compositor — nicht nachbauen.
- Layout-Namen exakt wie Upstream in der Szene: `picture-in-picture`, `dual-frame`, `vertical-stack`, `camera-only`, `no-webcam`; Anker `top-left, top, top-right, left, right, bottom-left, bottom, bottom-right`; Freistellung `none|transparent|blur|custom`.
- Kamera-Aufnahme: 30 fps, H.264 in MP4 (kein VP8/WebM), Bitrate-Leiter wie Upstream (`webcamBitrateFor`: ≥2160p 40 Mb/s, ≥1440p 24, ≥1080p 16, ≥720p 8, sonst 4). Qualitäten `1080p | 1440p | 2160p` als „ideal“, nie erzwungen; Standard **1080p** (Upstream 2160p — Abweichung siehe Entscheidungen). Datei nie gespiegelt aufnehmen.
- Konvention des Compositors: Kamera-Quellzeit = Bildschirm-Quellzeit − `webcamOffsetSec`. Unser Rekorder schreibt Versatz 0.
- Modell `selfie_segmentation_landscape.onnx`, 457 447 Bytes, sha256 wie in RECON §3, Apache-2.0, Eingabe 256×144. Es wird aus `C/public/mediapipe/selfie_segmentation/` erst nach `file`- und Prüfsummen-Kontrolle kopiert und liegt dann in `vendor/selfie-segmentation/` (Modell + LICENSE + README mit Herkunft); nichts anderes aus `C/` wird kopiert oder ausgeführt.
- Ein `ort` im ganzen Workspace (rc.12, statisch, `download-binaries`); kein `load-dynamic`, kein zweites ONNX Runtime.
- `EDITOR_VERSION` 3 → **4**; ein v3-Dokument öffnet unverändert (Layout `no-webcam` wenn das Projekt keine Kamera hat, sonst Upstream-Standard Bild-im-Bild). `PROJECT_SCHEMA` bleibt 1 (`camera` ist ein neues optionales Feld). Preset-Format bleibt `version: 1` (neues optionales Feld `camera`).
- Kamera nur nativ (AVFoundation im Rust-Kern), nie `getUserMedia`.
- `NSCameraUsageDescription` (en, im Stil der vorhandenen Usage-Strings) in **beiden** Bundle-Skripten und `com.apple.security.device.camera` in `macos/src-tauri/entitlements.plist` — **bevor** irgendein Code die Kamera anfasst (sonst beendet TCC den Prozess).
- Alle UI-Texte en+de; `.claude/rules/animation.md`; kein neues npm-Paket; neue Rust-Abhängigkeiten nur Feature-Erweiterungen vorhandener Crates (`objc2-av-foundation`) und `ort` im Compositor als `workspace = true`.
- Laufwerk knapp: `CARGO_INCREMENTAL=0`, `df -h /System/Volumes/Data` vor schweren Builds (Abbruch unter 1,5 GB). Compositor: `MAC_FFMPEG_DIR=third_party/ffmpeg/8.1.2`.

## Entscheidungen dieses Plans (Annahmen, prüfbar)

- **Versatz 0 statt eigener Zeitbasis:** Die Kamera startet schon während des Countdowns, ihre Bilder laufen durch dieselbe `PauseShared`-Uhr, der Writer startet seine Sitzung am `origin_ns` des Bildschirms. Bilder davor werden verworfen; fehlt zum Ursprung noch ein Bild, gilt das erste als ab 0 gehalten.
- **Kamera deckt immer die ganze Aufnahme ab:** `endSessionAtSourceTime(Bildschirm-Ende)`. Zusätzlich **Vendor-Patch 17**: der Export kürzt einen Clip nicht mehr auf das Kamera-Ende und bricht nicht ab, wenn die Kamera früher endet — er hält wie die Vorschau das letzte Bild. Ohne den Patch würden Vorschau und Export bei einer ausgefallenen Kamera verschieden aussehen (Anti-Ziel).
- **Kamera fällt mitten in der Aufnahme aus** (abgezogen, Laufzeitfehler, Unterbrechung) → Kamera endet, Datei wird sauber abgeschlossen, die Aufnahme läuft weiter, das HUD zeigt einmal „Kamera getrennt“. Kein Wiederverbinden (wie Upstream).
- **Standardqualität 1080p** statt Upstreams 2160p: Upstreams Wert ist unter Windows gemessen; hier laufen zwei Hardware-Encoder gleichzeitig (Bildschirm bis 4K/60 + Kamera). Wahl bleibt in den Einstellungen.
- **Freistellung auf unserem `ort`** (RECON §3 Option b) statt `ort`-Update oder Apple Vision: kein zweites Runtime, `cutout` bleibt unberührt, Spec sagt „MediaPipe via ONNX“.
- **Layout-Wechsel innerhalb des Videos gehört dazu** (Upstreams `layoutRegions`, Morph 0,2 s). Kürzel **L** = „Layout ab hier ändern“ (Upstream: C — bei uns ist C Zuschneiden).
- **Kamera-Stil in Presets** wie Upstream: Layout, Maske, Rundung, Spiegeln, Schrumpfen, Größe, Freistellung, Weichzeichnen, eigener Hintergrund — **nicht** Anker und Ausschnitt.
- **Eigener Kamera-Hintergrund** nutzt unsere eigenen Hintergründe (`Background`), nie Upstreams `wallpaper11.jpg`.
- **Keine Live-Kameravorschau im HUD** in dieser Etappe; das HUD zeigt Gerät und Zustand.

## Review Focus

1. **Kamera verweigert, nicht vorhanden oder während der Aufnahme abgezogen** → die Aufnahme startet bzw. läuft ohne Kamera weiter, Projekt ist öffnbar, Editor zeigt „ohne Kamera“, kein Absturz, kein Export-Fehler. (Task 2 `camera_denied_records_without_camera`, `camera_lost_mid_take_keeps_recording`; Task 5 `export_holds_last_camera_frame`.)
2. **Pause/Fortsetzen mit Kamera** → Kamera und Bildschirm bleiben deckungsgleich (gleiche Pausen herausgeschnitten, Dauer gleich ± 1 Bild). (Task 2 `camera_follows_pause_clock`, Live-Test mit zwei Pausen.)
3. **Projekt mit eingefügter Aufnahme (Etappe 4), von der nur eine Kamera hat** → je Clip richtiges Layout: der kameralose Clip wird `no-webcam`, der andere bleibt im gewählten Layout; Import kopiert `camera.mp4` mit. (Task 3 `per_clip_layout_without_camera`, Task 4 `scene_camera_paths_follow_source`, `import_take_copies_camera`.)
4. **Seitenverhältnis „Auto“ mit nebeneinander/übereinander und Gerätrahmen** → Ausgabe-Seitenverhältnis bezieht die Kamera ein, Zoom-Grenze nutzt das kleinere Bildschirmrechteck; Werte gleich den Upstream-Golden-Werten. (Task 3 `auto_aspect_includes_block_camera`, `zoom_limit_uses_block_screen_rect`.)
5. **Freistellung ohne Modell / Modell kaputt / Helfer ohne Segmentierung** → Kamera wird unverändert gezeichnet, ein Hinweis im Kamera-Tab, kein Helfer-Absturz, Export läuft. (Task 5 `missing_model_draws_camera_plain`.)

---

### Task 1: Kamera-Berechtigung, Geräteliste, Einstellungen, HUD

**Files:**
- Create: `core/rust-lib/src/record/camera_macos.rs` (Geräte + Berechtigung; Aufnahme kommt in Task 2), `vendor/openscreen-compositor/RECON-stage5.md` liegt schon (nicht ändern)
- Modify: `core/rust-lib/Cargo.toml` (`objc2-av-foundation`-Features `AVCaptureSession, AVCaptureDevice, AVCaptureInput, AVCaptureOutputBase, AVCaptureVideoDataOutput, AVCaptureSessionPreset`), `core/rust-lib/src/record/mod.rs`, `session.rs` (`RecordConfig`, `StartOptions`), `commands.rs`, `lib.rs`, `scripts/install-macos.sh`, `scripts/pack-macos-release-dmg.sh`, `macos/src-tauri/entitlements.plist`, `core/frontend/src/lib/ipc.ts`, `components/RecordHud.tsx`, `components/RecordSettingsSection.tsx`, Locales
- Test: `camera_macos.rs`/`session.rs`-Testmodule, `RecordHud.test.tsx`, `RecordSettingsSection.test.tsx`, ein Skript-Pin in `core/frontend/src/lib/record-bundle.test.ts` (liest beide Skripte + Entitlements von Platte)

**Interfaces (Produces):**
- `CameraQuality` (kebab `1080p|1440p|2160p`, Standard `1080p`) mit `fn ideal_size(self) -> (u32, u32)` (1920×1080 / 2560×1440 / 3840×2160) und `fn camera_bitrate(w: u32, h: u32) -> u32` (Leiter aus Global Constraints, gemessen an `min(w,h)` gegen 2160/1440/1080/720).
- `CameraDevice { id: String /*uniqueID*/, name: String, builtin: bool }`; `CameraAuth` (`authorized|denied|restricted|not-determined`).
- `RecordConfig` + `#[serde(default)] camera: bool` (aus), `camera_device: String` (leer = Standardgerät), `camera_quality: CameraQuality`. `StartOptions` + `#[serde(default)] camera: bool`, `camera_device: String`, `camera_quality: CameraQuality`.
- Rein + getestet: `fn pick_device<'a>(devices: &'a [CameraDevice], wanted: &str) -> Option<&'a CameraDevice>` (gewünschte id, sonst eingebaute, sonst erste; leere Liste → `None`).
- IPC: `record_camera_devices() -> Vec<CameraDevice>` (async), `record_camera_auth() -> CameraAuth`, `record_camera_request() -> CameraAuth` (async, `requestAccessForMediaType`, wartet auf den Rückruf), `record_open_camera_settings()` (`x-apple.systempreferences:com.apple.preference.security?Privacy_Camera`).
- HUD: Kamera-Schalter neben Mikrofon; Einschalten bei `not-determined` fragt nach, bei `denied` Hinweis + Knopf „Einstellungen öffnen“, Schalter bleibt aus. Geräteauswahl wenn > 1 Gerät. Einstellungen: Kamera an/aus, Gerät, Qualität.

- [ ] **Step 1: Failing tests:** `camera_bitrate_ladder` (3840×2160 → 40 000 000, 2560×1440 → 24 000 000, 1920×1080 → 16 000 000, 1280×720 → 8 000 000, 640×480 → 4 000 000) · `pick_device_prefers_wanted_then_builtin` · `pick_device_empty_is_none` · `old_config_without_camera_fields_loads` (JSON ohne die Felder → `camera: false`, Qualität 1080p) · TS: HUD-Schalter bei `denied` zeigt den Hinweis und bleibt aus; bei `not-determined` ruft er `record_camera_request` genau einmal · Skript-Pin: beide Skripte enthalten `NSCameraUsageDescription`, Entitlements `com.apple.security.device.camera`.
- [ ] **Step 2:** Implementieren. Usage-String über `plutil -insert` wie die vorhandenen (en). `AVCaptureDevice` nur über `devicesWithMediaType:AVMediaTypeVideo` bzw. DiscoverySession (eingebaut + extern); jede Objective-C-Stelle mit SAFETY-Notiz.
- [ ] **Step 3:** Tests grün (`cargo test -p inspector-rust-core --lib record::` + Vitest). Live-Probe (`#[ignore]` `camera_devices_live`): listet mindestens die eingebaute Kamera, Berechtigung lesbar. Commit `feat(record): camera permission, device list and settings`.

---

### Task 2: Kamera aufnehmen (`camera.mp4`) auf der gemeinsamen Uhr

**Files:**
- Modify: `core/rust-lib/src/record/camera_macos.rs`, `session.rs`, `project.rs`, `core.rs` (nur falls ein Helfer für „vor dem Ursprung“ fehlt), `editor.rs` (`TakeMeta`), `core/frontend/src/components/RecordHud.tsx` (Hinweis „Kamera getrennt“), Locales
- Test: `camera_macos.rs`, `session.rs`, `project.rs`-Testmodule

**Interfaces:**
- Consumes: Task 1 (`StartOptions.camera*`, `pick_device`, `camera_bitrate`).
- Produces: `pub const CAMERA_FILE: &str = "camera.mp4"`; `RecordingMeta` + `#[serde(default)] camera: Option<CameraMeta>`; `CameraMeta { width: u32, height: u32, fps: u32, offset_ms: i64 /*immer 0 von uns*/, device_name: String, ended_early_ms: Option<u64> /*Programmzeit, an der sie ausfiel*/ }`. `TakeMeta` + `camera: Option<(u32, u32)>` (Größe) und `camera_offset_ms: i64`; `TakeMeta::from_recording` füllt sie.
- `struct CameraCapture` mit `fn start(device: &CameraDevice, quality: CameraQuality, path: &Path, clock: Arc<PauseShared>) -> Result<CameraCapture, String>`, `fn set_origin(&self, origin_ns: u64)`, `fn finish(self, end_ns: u64) -> CameraOutcome` (`CameraOutcome { meta: Option<CameraMeta>, lost: bool }`); Zeitstempel: Probenzeit → `clock.offset_at` → verwerfen im Pausenfenster bzw. vor dem Ursprung, sonst `pts − origin − offset`, dann `VideoTimestampGate`.
- Rein + getestet: `fn camera_pts(sample_ns: u64, origin_ns: u64, clock: &PauseShared) -> Option<u64>` (dieselbe Regel wie der Bildschirm, `None` = verwerfen).
- Ablauf in `session.rs`: Kamera startet beim Start-Befehl (vor dem Countdown-Ende), bekommt nach dem ersten Bildschirmbild `set_origin`, stoppt **nach** der Bildschirmaufnahme mit `end_ns` = deren Ende (gleiche Stelle wie `mic.pad_to`). Fehler beim Start (verweigert, kein Gerät) = Warnung im Status, Aufnahme ohne Kamera. Event `record-camera-lost` einmal bei Ausfall; `AVCaptureVideoDataOutput` mit `alwaysDiscardsLateVideoFrames = true`, Verbindung **nicht** gespiegelt (`automaticallyAdjustsVideoMirroring = false`, `videoMirrored = false`).
- `session::import_take_into` kopiert `camera.mp4` mit, wenn vorhanden.

- [ ] **Step 1: Failing tests:** `camera_pts_drops_before_origin` · `camera_follows_pause_clock` (Probe in einer Pause → `None`, nach einer 2-s-Pause um 2 s verschoben) · `camera_denied_records_without_camera` (Start mit nicht verfügbarer Kamera über eine Test-Attrappe von `start` → `RecordingMeta.camera == None`, Status „ok mit Warnung“) · `camera_lost_mid_take_keeps_recording` (Attrappe meldet Ausfall → `ended_early_ms` gesetzt, Projekt `ready`) · `recording_meta_without_camera_field_loads` · `import_take_copies_camera`.
- [ ] **Step 2:** Implementieren. Die Session-Logik bekommt eine kleine Abstraktion (`trait CameraSink` o. ä.), damit die beiden Ausfall-Tests ohne echte Kamera laufen.
- [ ] **Step 3: Live-Test** (`#[ignore]` `session_live_camera_5s`, Anzeige wach, Kamera erlaubt): 5 s mit einer Pause von 1 s → `camera.mp4` existiert, `ffprobe`-freie Prüfung über `AVURLAsset`-Dauer: |Kamera − Bildschirm| ≤ 1/30 s; Bildrate ≈ 30; kein Spiegeln (erstes Bild gegen ein direkt per AVCapture geholtes Standbild nicht links-rechts vertauscht — oder dokumentiert, falls nicht prüfbar). Zwei Encoder gleichzeitig: CPU/Fehlerfreiheit im Commit nennen.
- [ ] **Step 4:** Tests grün. Commit `feat(record): record the camera into camera.mp4 on the shared clock`.

---

### Task 3: Layout-Port (Bild-im-Bild, nebeneinander, übereinander, nur Kamera)

**Files:**
- Modify: `core/rust-lib/src/record/layout.rs`, `scene_json.rs` (`screen_boxes`, Zoom-Grenze, Auto-Seitenverhältnis), `core/rust-lib/src/record/fixtures/` (neue Golden-Datei)
- Test: `layout.rs`-Testmodul, `scene_json.rs`-Testmodul

**Interfaces:**
- Produces (rein, `layout.rs`): `enum LayoutPreset { PictureInPicture, DualFrame, VerticalStack, CameraOnly, NoWebcam }` (serde kebab = Upstream-Namen) mit `fn resolve(self, has_camera: bool) -> Self` (ohne Kamera → `NoWebcam`), `fn is_block(self) -> bool`, `fn supports_reactive_zoom(self) -> bool` (nur PiP).
- `enum CameraAnchor` (8 Werte, Standard `BottomRight`) mit `fn fractions(self) -> (f64, f64)` (0/0.5/1). `enum CameraMask { Square, Rectangle }`.
- `struct CameraLook { preset: LayoutPreset, size_pct: f64, mask: CameraMask, roundness: f64, anchor: CameraAnchor }`.
- `struct CompositeLayout { screen: Rect, camera: Option<Rect>, screen_radius_frac: Option<f64>, camera_radius_frac: Option<f64>, screen_cover: bool, mask: CameraMask }` (Rechtecke in Bruchteilen der Ausgabe).
- `fn composite_layout(out: (u32, u32), screen_aspect: f64, camera: Option<(u32, u32)>, look: &CameraLook, padding_pct: f64, frame: Frame, even_border: bool) -> CompositeLayout` — Port von `C/src/lib/compositeLayout.ts:447-646` (Bild-im-Bild: Rand, Größe, Anker, Maske Quadrat = kürzere Seite, Radius `round(clamp01(roundness)·min(w,h)/2)`; Block-Layouts: Bildschirm + Kamera verschweißt, Spalt, Seitenverhältnis-Toleranz, Radius-Regel auf beide, `screenCover`, Maske Rechteck, unter einem Gerätrahmen am Rahmenkörper ausgerichtet; nur Kamera: ganze Fläche, Radius 0; ohne: wie heute `screen_rect`). Die heutige Funktion `screen_rect` bleibt der `NoWebcam`-Fall und liefert unverändert.
- `fn auto_aspect(screen_aspect, padding_pct, frame, preset: LayoutPreset) -> f64` (neuer Parameter; Upstream `restingCompositionAspect` + `autoFormatAspect`, `:362-445`); alle Aufrufer geben das aufgelöste Layout mit.
- `max_zoom_scale` bekommt für Block-Layouts das Bildschirmrechteck des Layouts (Upstream `sceneDescription.ts:1443-1449`).

- [ ] **Step 1: Golden-Werte erzeugen ohne Upstream-Code auszuführen:** die Rechenregeln aus `compositeLayout.ts` von Hand nachrechnen ist fehleranfällig — stattdessen die Testfälle aus Upstreams eigenen Tests übernehmen (`C/src/lib/compositeLayout.test.ts` lesen, Eingaben + erwartete Rechtecke abschreiben) in `fixtures/composite-layout-golden.json` mit Quelle je Fall. Gibt es für einen Fall keinen Upstream-Test, wird er aus der Regel hergeleitet und im Fixture als `derived` markiert.
- [ ] **Step 2: Failing tests:** `golden_cases_match_upstream` (alle Fixture-Fälle, Toleranz 1e-6 bzw. 1 px nach Rundung) · `pip_anchor_corners` (alle 8 Anker liegen am Rand mit Abstand `round(min(W,H)·0.02)`) · `square_mask_uses_short_side` · `per_clip_layout_without_camera` (`resolve(false)` → NoWebcam, Rechteck = bisheriges `screen_rect`) · `auto_aspect_includes_block_camera` · `zoom_limit_uses_block_screen_rect` · `no_webcam_unchanged` (alle bisherigen `layout.rs`-Tests unverändert grün).
- [ ] **Step 3:** Implementieren, Commit `feat(record): port the composite camera layouts`.

---

### Task 4: Editor-Dokument v4 (Kamera), Layout-Regionen, Szene, Presets

**Files:**
- Modify: `core/rust-lib/src/record/editor.rs`, `scene_json.rs`, `presets.rs`, `editor_ipc.rs` (TakeMetas tragen Kamera), `core/frontend/src/lib/editor.ts`, `core/frontend/src/lib/ipc.ts`
- Test: `editor.rs`, `scene_json.rs`, `presets.rs`-Testmodule, `editor.test.ts`

**Interfaces:**
- Consumes: Task 2 (`TakeMeta.camera`, `CAMERA_FILE`), Task 3 (`LayoutPreset`, `CameraAnchor`, `CameraMask`, `CameraLook`, `composite_layout`).
- Produces: `EDITOR_VERSION = 4`; `EditDoc` + `#[serde(default)] camera: CameraStyle`, `#[serde(default, deserialize_with = "lenient_vec")] layout_regions: Vec<LayoutRegion>`.
- `CameraStyle { preset: LayoutPreset, size_pct: f64 /*15..60, 40*/, mask: CameraMask /*Square*/, roundness: Option<f64> /*None = Standard der Maske 0.7/0.4*/, mirror: bool, reactive_zoom: bool /*true*/, anchor: CameraAnchor, crop: Option<CropRect> /*Bruchteile des Kamerabilds*/, background: CameraBackground }`; `CameraBackground { mode: CameraBgMode /*none|transparent|blur|custom*/, blur: f64 /*0..1, 0.5*/, custom: Background /*unsere Hintergründe*/ }`. `CameraStyle::default_for(has_camera: bool)` (ohne Kamera → `NoWebcam`).
- `LayoutRegion { id: String, clip: usize, start_ms: u64 /*SOURCE ms*/, preset: LayoutPreset }` — gilt ab `start_ms` bis zur nächsten Region desselben Clips; `normalize` behandelt sie wie Zooms (Clip-Klemme, Split, Löschen, Umordnen, Duplizieren ziehen mit).
- `migrate` v3 → v4: `camera = CameraStyle::default_for(projekt hat Kamera)`, `layout_regions` leer. `normalize`: Größe 15..60, Rundung 0..1, Weichzeichnen 0..1, Ausschnitt in 0..1 mit Mindestgröße 0.05.
- Szene (`scene_json.rs`, Felder exakt wie RECON §2a): je Clip `webcamPath` (Datei der Quelle unter `takes/<id>/` bzw. Projekt, leer ohne Kamera) und `webcamOffsetSec` (`offset_ms/1000`); `layout` mit `preset` (global aufgelöst), `webcamSize`, `webcamShape` (aufgelöst: `square|rectangle`), `webcamMirror`, `webcamPosition: null`, `webcamReactiveZoom` (nur wenn PiP), `webcamAnchor`, `webcamCrop`, `webcamRect`, `screenRect`, `screenRadiusFrac`, `webcamRadiusFrac`, `screenCover`, `layoutByClip` (je Clip aus `composite_layout` mit `resolve(has_camera)`); `layoutRegions` (Quellsekunden, `fromLayout`/`fromPreset`, `transitionStartSec`, `transitionSec = 0.2`, wie Upstream `sceneDescription.ts:1268-1309`); `webcamEffect` nur bei Modus ≠ none: `{mode, blurIntensity, background?, modelPath}` (Pfad liefert Task 5; bis dahin Schlüssel weglassen). Bestehender Vertrags-Pin `scene_json.rs` wird auf die neuen Werte umgeschrieben, ohne seine Absicht zu ändern.
- `screen_boxes` liefert das Bildschirmrechteck aus `composite_layout` (der Fokus-Kreuz-Overlay und Zoom-Planer nutzen es).
- Presets: `Preset` + `#[serde(default)] camera: Option<PresetCamera>` (alles aus `CameraStyle` außer `anchor` und `crop`); Speichern nimmt sie mit, Anwenden setzt sie (Anker/Ausschnitt bleiben), eigener Hintergrund wird wie andere Preset-Bilder kopiert.
- TS: Typen spiegeln, `setCamera(doc, patch)`, `addLayoutRegion(doc, clip, sourceMs, preset)`, `removeLayoutRegion`, Konstanten-Pin liest `editor.rs`.

- [ ] **Step 1: Failing tests:** `v3_doc_migrates_with_camera_default` (mit Kamera → PiP 40 % bottom-right; ohne → no-webcam) · `layout_region_follows_split_and_reorder` · `scene_camera_paths_follow_source` · `scene_no_camera_clip_is_no_webcam` · `scene_layout_regions_have_transitions` (0.2 s, `fromPreset` = vorige Region) · `scene_effect_absent_when_none` · `preset_roundtrip_keeps_anchor_and_crop` · `old_preset_without_camera_applies` · TS-Operationen.
- [ ] **Step 2:** Implementieren. Commit `feat(record): editor document v4 with camera style and layout regions`.

---

### Task 5: Helfer — Freistellung auf unserem `ort`, Kamera-Länge, Modell im Bundle

**Files:**
- Modify: `vendor/openscreen-compositor/Cargo.toml`, `vendor/openscreen-compositor/src/segmentation.rs`, `vendor/openscreen-compositor/src/timeline_walk.rs`, `vendor/openscreen-compositor/VENDORED.md`, `record-render/Cargo.toml` (Feature `segmentation`), `record-render/src/session.rs` (Modellpfad aus Umgebung `IR_SEGMENTATION_MODEL`, Standard `Contents/Resources/selfie_segmentation_landscape.onnx` neben dem Helfer), `core/rust-lib/src/record/scene_json.rs` (`modelPath`), `core/rust-lib/src/record/render_client.rs` (Umgebung beim Start), `scripts/bundle-record-render.sh`, `core/frontend/src/lib/ipc.ts` (`editor_segmentation_available`)
- Create: `vendor/selfie-segmentation/{selfie_segmentation_landscape.onnx, LICENSE, README.md}`
- Test: `segmentation.rs`-Tests (ein vorhandener ignorierter Test bekommt den neuen Pfad), `timeline_walk.rs`-Test, `record-render/tests/helper_live.rs`, `scene_json.rs`

**Interfaces:**
- Consumes: Task 4 (`webcamEffect`-Emission, `CameraBgMode`).
- Produces: **Vendor-Patch 16** — Compositor `ort = { workspace = true, optional = true }`, Feature `segmentation = ["dep:ort"]` nur unter dem macOS-arm64-cfg; `runtime_available()` = true mit Feature; Testpfad zeigt auf `vendor/selfie-segmentation/`. **Vendor-Patch 17** — `timeline_walk.rs`: Clip-Ende nicht mehr auf `Kamera-Ende + Versatz` kürzen und keinen Fehler, wenn die Kamera vor dem Clipanfang endet; der Kamera-Decoder hält sein letztes Bild (wie `live.rs`), ohne Bild wird keine Kamera gezeichnet. Beide in `VENDORED.md` mit Begründung.
- IPC `editor_segmentation_available() -> bool` (Helfer meldet nach `Open`, ob Modell geladen werden kann; Event-Feld oder Antwortfeld — Implementierer wählt, Rust entscheidet).
- Bundle: Modell nach `Contents/Resources/`, `LICENSE`/`README` nach `Contents/Resources/THIRD-PARTY/selfie-segmentation/`; das Skript bricht ab, wenn Größe oder Prüfsumme nicht stimmen.

- [ ] **Step 1:** Modell kopieren: `file` zeigt Daten, Größe 457 447, sha256 = RECON §3 — sonst Abbruch. README nennt Herkunft (MediaPipe `selfie_segmentation_landscape.tflite`, konvertiert von OpenScreen v2.1.2), Lizenz Apache-2.0.
- [ ] **Step 2: Failing tests:** `segmenter_runs_on_workspace_ort` (Modell laden, 256×144 einfarbiges Bild → Maske 256×144, Werte 0..255; läuft, nicht ignoriert, nur macOS-arm64) · `export_holds_last_camera_frame` (Clip länger als die Kamera → kein Fehler, Exportdauer = Clipdauer; mit synthetischen kurzen Dateien oder als Live-Test, falls ohne echte Datei nicht prüfbar) · `missing_model_draws_camera_plain` (Szene mit `modelPath` auf eine nicht existierende Datei → Bild ohne Fehler, Kamera unverändert) · `scene_effect_carries_model_path`.
- [ ] **Step 3:** Implementieren. Größe des Helfers vorher/nachher messen und im Commit nennen.
- [ ] **Step 4: Live-Test** (`helper_live.rs`, `IR_RECORD_PROJECT` = Projekt mit Kamera aus Task 2): Vorschau-Standbild gegen Export-Standbild für Bild-im-Bild, nebeneinander, nur Kamera, Freistellung Weichzeichnen — mittlere Abweichung < 3, schlimmster 32-px-Block < 20 (Maßstab wie Etappe 4).
- [ ] **Step 5:** `MAC_FFMPEG_DIR=third_party/ffmpeg/8.1.2 CARGO_INCREMENTAL=0 cargo test -p openscreen-compositor --lib` + `-p record-render` + Core grün. Commits `fix(vendor): run webcam segmentation on the workspace ort (patch 16)` und `fix(vendor): export holds the last camera frame like the preview (patch 17)`.

---

### Task 6: Editor-Oberfläche — Kamera-Tab, Layout-Spur, Kürzel L

**Files:**
- Create: `core/frontend/src/components/editor/CameraTab.tsx`, `core/frontend/src/components/editor/CameraFraming.tsx`, `core/frontend/src/lib/camera-layout.ts` (reine UI-Logik)
- Modify: `components/editor/Inspector.tsx` (Tab `camera`), `components/editor/Timeline.tsx` (Layout-Spur), `lib/editor-shortcuts.ts` (`layoutHere: { key: "l" }`), `components/ShortcutsDialog.tsx`, `components/RecordEditor.tsx`, `components/editor/PresetsMenu.tsx` (Kamera-Teil), Locales
- Test: `CameraTab.test.tsx`, `camera-layout.test.ts`, `Timeline.test.tsx`, `editor-shortcuts.test.ts`

**Interfaces:**
- Consumes: Task 4 (`setCamera`, `addLayoutRegion`, `removeLayoutRegion`, Typen), Task 5 (`editor_segmentation_available`).
- Produces: Kamera-Tab mit — Layout-Auswahl (5 Symbole), „ganzes Video“ vs. „ab hier“ wenn Regionen existieren; Spiegeln; „Beim Zoom verkleinern“ nur bei Bild-im-Bild; Maske Quadrat/Rechteck + Rundung 0–100 % (Rücksetzen auf den Standard der Maske, solange nicht angefasst); Größe 15–60 %, Schritt 1 (Bild-im-Bild); Anker als 3×3-Raster ohne Mitte; Freistellung (Chips none/transparent/blur/custom, gesperrt mit Hinweis wenn `editor_segmentation_available` false), Weichzeichnen 0–100 %, eigener Hintergrund (vorhandene Hintergrund-Auswahl wiederverwenden); Ausschnitt (`CameraFraming`: Zoom + Verschieben auf einem Kamera-Standbild aus der Filmstreifen-/Frame-IPC, ergibt `crop`). Ohne Kamera im Projekt: alles gesperrt + Hinweis „Diese Aufnahme hat keine Kamera“.
- Layout-Spur in der Zeitleiste (unter der Zoom-Spur, nur wenn das Projekt eine Kamera hat): Blöcke je Region, Klick wählt, ⌫ löscht; **L** setzt eine Region am Abspielkopf mit dem aktuellen Layout und öffnet den Kamera-Tab. Ziehen eines Blockanfangs = verschieben mit Einrasten (8 px, Regeln aus Etappe 4).
- `camera-layout.ts`: `roundnessDefault(mask)`, `effectiveRoundness(style)`, `canShrink(preset)`, `regionAt(doc, programmeMs)` — reine Funktionen, getestet.

- [ ] **Step 1: Failing tests:** `roundness_resets_with_mask_until_touched` · `shrink_toggle_only_for_pip` · `segmentation_unavailable_disables_chips` · `no_camera_disables_tab` · `l_adds_region_at_playhead` · Kürzel-Kollisionstest (L kollidiert mit keinem Standard) · Timeline: Region ziehen rastet an Clip-Kante ein.
- [ ] **Step 2:** Implementieren, Animationen nur nach `.claude/rules/animation.md`. Mockup (`mockup.html`) bekommt ein erfundenes Projekt mit Kamera, damit der Tab sichtbar geprüft werden kann; Breite 700 px in en und de ansehen (Bilder nicht ins Repo).
- [ ] **Step 3:** Vitest + `bash scripts/check.sh` grün. Commit `feat(record): camera tab, layout track and the L shortcut`.

---

### Task 7: Doku, Version, Installation, Ende-zu-Ende

**Files:**
- Modify: `features.txt`, `CHANGELOG.md`, `CLAUDE.md` (neuer Abschnitt „record — stage 5“), `README.md`, `README.de.md`, `core/frontend/src/lib/commandDocs.ts` (record-Doc: Kamera erwähnen), `feature-extras.ts` falls Kürzel dort stehen, Versionsdateien (0.232.0), `docs/superpowers/specs/2026-11-01-record-design.md` (Korrekturen, falls die Umsetzung von der Spec abweicht: Standardqualität 1080p)

- [ ] **Step 1:** Doku nachziehen (Doku-Vertrag aus CLAUDE.md: `features.txt`-Zeile, CHANGELOG englisch ohne Geheimnisse, beide READMEs über `gen-docs`), Version 0.232.0 überall.
- [ ] **Step 2:** `bash scripts/check.sh` + `pnpm test` grün; Windows-Cross-Check (`ORT_SKIP_DOWNLOAD=1 cargo check -p inspector-rust-core --target x86_64-pc-windows-gnu`, keine neuen Diagnosen).
- [ ] **Step 3:** `scripts/install-macos.sh`; im installierten Bundle prüfen: `Info.plist` enthält `NSCameraUsageDescription`, Modell + Lizenz liegen in `Resources`, `otool -L` des App-Binaries ohne libav. Ende-zu-Ende ohne Tastatureingaben in fremde Fenster: Live-Test aus Task 2 erzeugt ein Projekt mit Kamera, Live-Test aus Task 5 exportiert es.
- [ ] **Step 4:** Commit `docs(record): stage 5 — webcam (v0.232.0)`, Push.
