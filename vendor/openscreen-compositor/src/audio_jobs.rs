//! Décodage et étirement de l'audio d'un clip, en parallèle du parcours vidéo.
//!
//! Les trois pipelines faisaient ce travail **dans** le callback `on_clip_end` de
//! `walk_composited_timeline`, donc sur le thread de rendu et entre deux clips. Rien
//! n'appelle `progress()` pendant ce temps : la barre d'export s'arrêtait sur le
//! pourcentage de la dernière frame du clip et y restait pour toute la durée du décodage
//! et de l'étirement. C'est la moitié « reporting » du « figé à ~80 % » — la moitié
//! « coût » a été traitée par le passage à atempo, mais un clip long, un repli WSOLA ou
//! n'importe quelle étape audio future reproduisent le symptôme à l'identique.
//!
//! Y répondre en publiant une progression pendant cette phase aurait demandé de changer le
//! protocole natif → JS (il ne transporte qu'un compteur de frames absolu) et de répartir
//! un total que les deux côtés calculent séparément. Déplacer le travail est plus simple et
//! strictement meilleur : l'audio d'un clip ne dépend que de ce clip, il n'y a donc aucune
//! raison qu'il occupe le thread qui compose les frames du clip suivant. Le parcours vidéo
//! continue de rapporter sa progression sans interruption, et le temps audio disparaît du
//! mur d'export au lieu d'y être seulement mieux affiché — ce que
//! `export-pipeline.md` prétendait déjà.
//!
//! Chaque job ouvre son propre `AVFormatContext` sur le fichier du clip : libavformat
//! n'a pas d'état partagé entre contextes, et le décodeur vidéo du parcours en a un autre
//! sur le même chemin, en lecture seule lui aussi.

use crate::audio::{
    apply_gain_db, decode_clip_audio, decode_extra_audio_window, loudness_gain_db,
    stretch_clip_pcm_by_speed, PlanarPcm,
};
use crate::scene::ExtraAudio;
use crate::regions::SpeedSegment;
use std::collections::VecDeque;
use std::thread::JoinHandle;

/// Nombre de jobs audio en vol.
///
/// Un thread par clip serait sans plafond : une timeline de deux cents clips décoderait
/// deux cents pistes à la fois, chacune avec son contexte ffmpeg et son PCM complet en
/// mémoire. Quatre suffisent à couvrir le décodage d'un clip par le rendu du suivant, qui
/// est tout ce qu'on cherche ici.
const MAX_INFLIGHT_AUDIO_JOBS: usize = 4;

/// Le corps d'un job : décode la fenêtre gardée du clip, l'étire sur ses spans de vitesse et
/// l'amène au niveau de loudness cible avec le gain mesuré sur le fichier entier
/// (`loudness_gain_db`, celui que la preview applique aussi).
///
/// Rend `None` quand le clip se déclare audio mais n'a pas de flux décodable, ou quand le
/// décodage échoue — dans les deux cas l'export continue et le clip sort muet, comme avant
/// que ce travail passe sur un thread. Les deux messages sont les mêmes qu'alors ; ils
/// sortent seulement d'un autre thread.
pub fn decode_and_stretch_clip_audio(
    clip_index: usize,
    screen_path: &str,
    source_start_sec: f64,
    source_end_sec: f64,
    speed_segments: &[SpeedSegment],
    out_fps: f64,
) -> Option<PlanarPcm> {
    match decode_clip_audio(screen_path, source_start_sec, source_end_sec) {
        Ok(Some(pcm)) => {
            let mut pcm = stretch_clip_pcm_by_speed(&pcm, speed_segments, out_fps);
            apply_gain_db(&mut pcm, loudness_gain_db(screen_path));
            Some(pcm)
        }
        Ok(None) => {
            eprintln!(
                "[pipeline] warning: clip #{clip_index} déclaré audio mais sans flux décodable; silence conservé"
            );
            None
        }
        Err(error) => {
            eprintln!(
                "[pipeline] warning: décodage audio du clip #{clip_index} échoué ({error:#}); silence conservé"
            );
            None
        }
    }
}

/// Inspector Rust: `decode_and_stretch_clip_audio` plus an optional extra track (`mic.wav`).
///
/// The extra track is decoded over the SAME source window (shifted by its offset), its gain
/// applied, and summed with the clip audio BEFORE the stretch — so a speed region stretches
/// both alike and the mic stays lip-synced. Clip audio keeps its loudness normalisation (a
/// linear gain, so applying it before the stretch equals applying it after). Either part may
/// be absent; `None` only when neither yields audio.
#[allow(clippy::too_many_arguments)]
pub fn decode_and_stretch_clip_audio_with_extra(
    clip_index: usize,
    screen_path: &str,
    has_audio: bool,
    extra: Option<&ExtraAudio>,
    source_start_sec: f64,
    source_end_sec: f64,
    speed_segments: &[SpeedSegment],
    out_fps: f64,
) -> Option<PlanarPcm> {
    let clip = if has_audio {
        match decode_clip_audio(screen_path, source_start_sec, source_end_sec) {
            Ok(Some(mut pcm)) => {
                apply_gain_db(&mut pcm, loudness_gain_db(screen_path));
                Some(pcm)
            }
            Ok(None) => {
                eprintln!(
                    "[pipeline] warning: clip #{clip_index} déclaré audio mais sans flux décodable; silence conservé"
                );
                None
            }
            Err(error) => {
                eprintln!(
                    "[pipeline] warning: décodage audio du clip #{clip_index} échoué ({error:#}); silence conservé"
                );
                None
            }
        }
    } else {
        None
    };
    let extra = extra.and_then(|extra| decode_extra_audio_window(extra, source_start_sec, source_end_sec));
    let mixed = match (clip, extra) {
        (None, None) => return None,
        (Some(pcm), None) | (None, Some(pcm)) => pcm,
        (Some(mut pcm), Some(extra)) => {
            for (plane, extra_plane) in pcm.iter_mut().zip(extra.iter()) {
                if plane.len() < extra_plane.len() {
                    plane.resize(extra_plane.len(), 0.0);
                }
                for (sample, add) in plane.iter_mut().zip(extra_plane.iter()) {
                    *sample += *add;
                }
            }
            pcm
        }
    };
    Some(stretch_clip_pcm_by_speed(&mixed, speed_segments, out_fps))
}

/// Collecte les résultats de jobs indexés lancés au fil du parcours, en bornant le nombre
/// de threads simultanés.
///
/// L'ordre de restitution est celui des index, pas celui d'achèvement : `into_results` rend
/// un `Vec` de la taille annoncée où chaque case porte le résultat de son clip.
pub struct ClipAudioJobs<T> {
    inflight: VecDeque<(usize, JoinHandle<T>)>,
    results: Vec<Option<T>>,
}

impl<T: Send + 'static> ClipAudioJobs<T> {
    pub fn new(clip_count: usize) -> Self {
        Self {
            inflight: VecDeque::new(),
            results: (0..clip_count).map(|_| None).collect(),
        }
    }

    /// Lance `job` pour `clip_index`. Si le plafond est atteint, attend d'abord le plus
    /// ancien job en vol — celui qui a eu le plus de temps pour finir.
    pub fn spawn(&mut self, clip_index: usize, job: impl FnOnce() -> T + Send + 'static) {
        while self.inflight.len() >= MAX_INFLIGHT_AUDIO_JOBS {
            self.collect_oldest();
        }
        self.inflight
            .push_back((clip_index, std::thread::spawn(job)));
    }

    /// Attend tous les jobs restants et rend les résultats rangés par index de clip.
    pub fn into_results(mut self) -> Vec<Option<T>> {
        while !self.inflight.is_empty() {
            self.collect_oldest();
        }
        // `mem::take` et pas un move : le `Drop` ci-dessous interdit de sortir un champ de
        // `self`. Il ne trouvera plus rien à joindre, la file étant vide.
        std::mem::take(&mut self.results)
    }

    fn collect_oldest(&mut self) {
        let Some((clip_index, handle)) = self.inflight.pop_front() else {
            return;
        };
        match handle.join() {
            Ok(value) => {
                if let Some(slot) = self.results.get_mut(clip_index) {
                    *slot = Some(value);
                }
            }
            // Un panic dans un job audio ne doit pas emporter l'export : le clip sort
            // muet, comme il le faisait déjà quand `decode_clip_audio` échouait.
            Err(_) => eprintln!(
                "[pipeline] warning: le job audio du clip #{clip_index} a paniqué; silence conservé"
            ),
        }
    }
}

/// Un `JoinHandle` droppé **détache** son thread. Entre le premier `spawn` et
/// `into_results` il y a des `?` — le parcours lui-même, le flush de l'encodeur — et sur
/// l'un d'eux la collection partait en fumée en laissant jusqu'à quatre décodages en vol
/// dans un addon natif que l'hôte peut décharger. On joint donc à la destruction : rien ne
/// survit à la portée, chemin d'erreur compris.
///
/// Ce n'est pas une annulation : `decode_clip_audio` est un appel opaque et long, et
/// l'interrompre demanderait de lui passer un `AVIOInterruptCB` — un autre changement, dans
/// un autre fichier. L'attente est bornée par le plus lent des quatre, soit quelques
/// secondes depuis que le stretch passe par atempo, et elle ne coûte que sur un export qui
/// a déjà échoué.
impl<T> Drop for ClipAudioJobs<T> {
    fn drop(&mut self) {
        for (clip_index, handle) in std::mem::take(&mut self.inflight) {
            if handle.join().is_err() {
                eprintln!(
                    "[pipeline] warning: le job audio du clip #{clip_index} a paniqué pendant l'abandon de l'export"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn results_are_indexed_by_clip_not_by_completion_order() {
        // Le premier job est le plus lent : si on rangeait par ordre d'achèvement, le PCM
        // du clip 0 atterrirait sur le clip 2 et l'export monterait l'audio dans le
        // désordre sans rien signaler.
        let mut jobs = ClipAudioJobs::new(3);
        jobs.spawn(0, || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            "zero"
        });
        jobs.spawn(1, || "one");
        jobs.spawn(2, || "two");
        assert_eq!(
            jobs.into_results(),
            vec![Some("zero"), Some("one"), Some("two")]
        );
    }

    #[test]
    fn a_clip_without_a_job_keeps_its_empty_slot() {
        // Les clips sans audio ne lancent rien ; leur case doit rester `None` pour que
        // `assemble_concatenated_pcm` y mette du silence.
        let mut jobs = ClipAudioJobs::new(3);
        jobs.spawn(1, || 7u32);
        assert_eq!(jobs.into_results(), vec![None, Some(7), None]);
    }

    #[test]
    fn never_more_than_the_cap_run_at_once() {
        // Sans plafond, une timeline longue ouvrirait un contexte ffmpeg et un PCM complet
        // par clip, tous en même temps.
        let live = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut jobs = ClipAudioJobs::new(32);
        for index in 0..32 {
            let live = Arc::clone(&live);
            let peak = Arc::clone(&peak);
            jobs.spawn(index, move || {
                let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(5));
                live.fetch_sub(1, Ordering::SeqCst);
                index
            });
        }
        let results = jobs.into_results();
        assert_eq!(results.len(), 32);
        assert!(results.iter().enumerate().all(|(i, r)| *r == Some(i)));
        assert!(
            peak.load(Ordering::SeqCst) <= MAX_INFLIGHT_AUDIO_JOBS,
            "jusqu'à {} jobs simultanés pour un plafond de {MAX_INFLIGHT_AUDIO_JOBS}",
            peak.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn dropping_the_collection_joins_its_jobs_instead_of_detaching_them() {
        // Le chemin d'erreur : entre le premier `spawn` et `into_results` il y a des `?`.
        // Sans le `Drop`, jusqu'à quatre décodages continuaient dans le vide après l'abandon
        // de l'export, dans un addon que l'hôte peut décharger.
        let finished = Arc::new(AtomicUsize::new(0));
        {
            let mut jobs = ClipAudioJobs::new(4);
            for index in 0..4 {
                let finished = Arc::clone(&finished);
                jobs.spawn(index, move || {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    finished.fetch_add(1, Ordering::SeqCst);
                });
            }
            // Pas d'`into_results` : on abandonne, comme le ferait un `?`.
        }
        assert_eq!(
            finished.load(Ordering::SeqCst),
            4,
            "des jobs tournaient encore après la destruction de la collection"
        );
    }

    #[test]
    fn a_panicking_job_leaves_its_clip_silent_without_taking_the_export_down() {
        let mut jobs = ClipAudioJobs::new(2);
        jobs.spawn(0, || panic!("décodage impossible"));
        jobs.spawn(1, || 42u32);
        assert_eq!(jobs.into_results(), vec![None, Some(42)]);
    }

    // --- Extra audio (Inspector Rust: mic.wav per clip, mixed before the speed stretch) ---

    use crate::audio::{PlanarPcm, AUDIO_OUTPUT_SAMPLE_RATE};
    use crate::regions::speed_segments_for_window;
    use crate::scene::{ExtraAudio, SceneSpeedRegion};

    const RATE: f64 = AUDIO_OUTPUT_SAMPLE_RATE as f64;

    /// A 16-bit stereo 48 kHz WAV of `pcm` (same layout as `audio::tests::write_wav`).
    fn write_wav(path: &std::path::Path, pcm: &PlanarPcm) {
        let frames = pcm[0].len();
        let data_len = (frames * 4) as u32;
        let mut bytes = Vec::with_capacity(44 + frames * 4);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for index in 0..frames {
            for plane in pcm {
                let value = (plane[index].clamp(-1.0, 1.0) * 32767.0).round() as i16;
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        std::fs::write(path, bytes).unwrap();
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ir-extra-audio-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// `secs` of silence with a 2 ms click of 0.9 at `click_sec`.
    fn click_track(secs: f64, click_sec: f64) -> PlanarPcm {
        let mut plane = vec![0.0f32; (secs * RATE).round() as usize];
        let at = (click_sec * RATE).round() as usize;
        for sample in plane.iter_mut().skip(at).take(96) {
            *sample = 0.9;
        }
        vec![plane.clone(), plane]
    }

    fn peak_sec(pcm: &PlanarPcm) -> f64 {
        let (index, _) = pcm[0]
            .iter()
            .enumerate()
            .fold((0, 0.0f32), |best, (i, v)| if v.abs() > best.1 { (i, v.abs()) } else { best });
        index as f64 / RATE
    }

    fn peak(pcm: &PlanarPcm) -> f32 {
        pcm.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    const FPS: f64 = 30.0;

    #[test]
    fn mic_follows_speed_region() {
        // The mic must be stretched exactly like the picture: a click at source 3.5 s, behind
        // a 2× region over 1–3 s, lands at programme 1 + 1 + 0.5 = 2.5 s. Placed as an
        // external track it would stay at 3.5 s — a full second out of sync.
        let dir = temp_dir("speed");
        let mic = dir.join("mic.wav");
        write_wav(&mic, &click_track(4.0, 3.5));
        let regions = [SceneSpeedRegion { clip_index: Some(0), start_sec: 1.0, end_sec: 3.0, speed: 2.0 }];
        let segments = speed_segments_for_window(&regions, 0.0, 4.0, 0.0, FPS);
        let extra = ExtraAudio { path: mic.to_string_lossy().into_owned(), offset_sec: 0.0, gain_db: 0.0 };
        let pcm = decode_and_stretch_clip_audio_with_extra(
            0, "", false, Some(&extra), 0.0, 4.0, &segments, FPS,
        )
        .expect("mic present → audio");
        assert!((pcm[0].len() as f64 / RATE - 3.0).abs() < 0.05, "programme is 3 s long");
        let at = peak_sec(&pcm);
        assert!((at - 2.5).abs() <= 0.010, "click at {at} s, expected 2.5 s ±10 ms");
    }

    #[test]
    fn mic_gain_applies() {
        let dir = temp_dir("gain");
        let mic = dir.join("mic.wav");
        let total = (2.0 * RATE) as usize;
        let plane: Vec<f32> = (0..total)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin())
            .collect();
        write_wav(&mic, &vec![plane.clone(), plane]);
        let segments = speed_segments_for_window(&[], 0.0, 2.0, 0.0, FPS);
        let run = |gain_db: f64| {
            let extra = ExtraAudio { path: mic.to_string_lossy().into_owned(), offset_sec: 0.0, gain_db };
            peak(&decode_and_stretch_clip_audio_with_extra(0, "", false, Some(&extra), 0.0, 2.0, &segments, FPS).unwrap())
        };
        let ratio = run(-6.0) / run(0.0);
        assert!((ratio - 0.501).abs() <= 0.501 * 0.01, "−6 dB gave ×{ratio}");
        // Clamped to −60…+12 even if the scene asks for more.
        let loud = run(40.0) / run(0.0);
        assert!((loud - 3.981).abs() <= 3.981 * 0.01, "+40 dB must clamp to +12 dB, got ×{loud}");
    }

    #[test]
    fn mic_offset_shifts() {
        // mic time = source time − offset: the mic started 0.25 s after the video, so a click
        // at mic 1.0 s belongs to source 1.25 s. The first 0.25 s of the clip have no mic.
        let dir = temp_dir("offset");
        let mic = dir.join("mic.wav");
        write_wav(&mic, &click_track(2.0, 1.0));
        let segments = speed_segments_for_window(&[], 0.0, 2.0, 0.0, FPS);
        let extra = ExtraAudio { path: mic.to_string_lossy().into_owned(), offset_sec: 0.25, gain_db: 0.0 };
        let pcm = decode_and_stretch_clip_audio_with_extra(0, "", false, Some(&extra), 0.0, 2.0, &segments, FPS)
            .unwrap();
        assert!((pcm[0].len() as f64 / RATE - 2.0).abs() < 0.05);
        let at = peak_sec(&pcm);
        assert!((at - 1.25).abs() <= 0.010, "click at {at} s, expected 1.25 s");
        // A clip that starts later in the source: window 0.5–2.0 → click at programme 0.75 s.
        let segments = speed_segments_for_window(&[], 0.5, 2.0, 0.0, FPS);
        let pcm = decode_and_stretch_clip_audio_with_extra(0, "", false, Some(&extra), 0.5, 2.0, &segments, FPS)
            .unwrap();
        let at = peak_sec(&pcm);
        assert!((at - 0.75).abs() <= 0.010, "click at {at} s, expected 0.75 s");
    }

    #[test]
    fn missing_extra_audio_is_silence_not_error() {
        let dir = temp_dir("missing");
        let extra = ExtraAudio {
            path: dir.join("does-not-exist.wav").to_string_lossy().into_owned(),
            offset_sec: 0.0,
            gain_db: 0.0,
        };
        let segments = speed_segments_for_window(&[], 0.0, 2.0, 0.0, FPS);
        // No clip audio either: nothing to mix, the clip stays silent (assemble pads it).
        assert!(decode_and_stretch_clip_audio_with_extra(0, "", false, Some(&extra), 0.0, 2.0, &segments, FPS).is_none());
        // With clip audio: the clip audio survives, the missing mic adds nothing.
        let screen = dir.join("screen.wav");
        write_wav(&screen, &click_track(2.0, 0.5));
        let pcm = decode_and_stretch_clip_audio_with_extra(
            0, &screen.to_string_lossy(), true, Some(&extra), 0.0, 2.0, &segments, FPS,
        )
        .expect("clip audio still decodes");
        assert!((peak_sec(&pcm) - 0.5).abs() <= 0.010);
        // Garbage file: same rule.
        let garbage = dir.join("garbage.wav");
        std::fs::write(&garbage, b"not a wav").unwrap();
        let extra = ExtraAudio { path: garbage.to_string_lossy().into_owned(), offset_sec: 0.0, gain_db: 0.0 };
        assert!(decode_and_stretch_clip_audio_with_extra(0, "", false, Some(&extra), 0.0, 2.0, &segments, FPS).is_none());
    }

    #[test]
    fn mic_window_entirely_before_the_file_is_silence_of_full_length() {
        // offset larger than the clip end: every mic time is negative → silence, but the
        // window keeps its length so the concat plan stays aligned.
        let dir = temp_dir("before");
        let mic = dir.join("mic.wav");
        write_wav(&mic, &click_track(1.0, 0.5));
        let extra = ExtraAudio { path: mic.to_string_lossy().into_owned(), offset_sec: 5.0, gain_db: 0.0 };
        let pcm = crate::audio::decode_extra_audio_window(&extra, 0.0, 2.0).expect("silence, not None");
        assert_eq!(pcm[0].len(), (2.0 * RATE) as usize);
        assert_eq!(peak(&pcm), 0.0);
    }

    #[test]
    fn extra_audio_gain_is_clamped() {
        use crate::audio::clamp_extra_gain_db;
        assert_eq!(clamp_extra_gain_db(-100.0), -60.0);
        assert_eq!(clamp_extra_gain_db(50.0), 12.0);
        assert_eq!(clamp_extra_gain_db(-3.0), -3.0);
        assert_eq!(clamp_extra_gain_db(f64::NAN), 0.0);
    }
}
