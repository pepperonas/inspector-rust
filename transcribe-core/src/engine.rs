//! whisper.cpp (whisper-rs, Metal) behind a small API: one context, one state
//! per source stream.

use crate::lang::{accept, mean_logprob, pick_de_en, resolve_lang};
use crate::decode::{DecodeOpts, Decoded};
use crate::queue::JobKind;
use crate::text::drop_loops;
use std::path::Path;
use crate::words::{merge_utf8, DtwPreset, TokDecoded, WordTranscriber};
use whisper_rs::{
    DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
    WhisperState,
};

pub struct Engine {
    ctx: WhisperContext,
    de_id: usize,
    en_id: usize,
}

pub struct Stream {
    state: WhisperState,
    eot: i32,
    de_id: usize,
    en_id: usize,
}

static HOOKS: std::sync::Once = std::sync::Once::new();

impl Engine {
    pub fn load(path: &Path) -> Result<Engine, String> {
        HOOKS.call_once(whisper_rs::install_logging_hooks);
        let mut p = WhisperContextParameters::default();
        p.use_gpu(true).flash_attn(true);
        let ctx = WhisperContext::new_with_params(path.to_string_lossy().as_ref(), p)
            .map_err(|e| format!("load {}: {e}", path.display()))?;
        let de_id = whisper_rs::get_lang_id("de").ok_or("no de language id")? as usize;
        let en_id = whisper_rs::get_lang_id("en").ok_or("no en language id")? as usize;
        Ok(Engine { ctx, de_id, en_id })
    }

    /// A second context for word timing: DTW token times (`t_dtw`) need
    /// flash attention OFF, so this is never the live/meeting engine. Loaded
    /// per word job and dropped afterwards. `preset` must match the model
    /// (`words::dtw_preset_for`).
    pub fn load_dtw(path: &Path, preset: DtwPreset) -> Result<Engine, String> {
        let model_preset = match preset {
            DtwPreset::LargeV3Turbo => DtwModelPreset::LargeV3Turbo,
            DtwPreset::Medium => DtwModelPreset::Medium,
            DtwPreset::Small => DtwModelPreset::Small,
        };
        HOOKS.call_once(whisper_rs::install_logging_hooks);
        let mut p = WhisperContextParameters::default();
        p.use_gpu(true).flash_attn(false).dtw_parameters(DtwParameters {
            mode: DtwMode::ModelPreset { model_preset },
            ..Default::default()
        });
        let ctx = WhisperContext::new_with_params(path.to_string_lossy().as_ref(), p)
            .map_err(|e| format!("load {}: {e}", path.display()))?;
        let de_id = whisper_rs::get_lang_id("de").ok_or("no de language id")? as usize;
        let en_id = whisper_rs::get_lang_id("en").ok_or("no en language id")? as usize;
        Ok(Engine { ctx, de_id, en_id })
    }

    pub fn new_stream(&self) -> Result<Stream, String> {
        let state = self.ctx.create_state().map_err(|e| format!("state: {e}"))?;
        Ok(Stream { state, eot: self.ctx.token_eot(), de_id: self.de_id, en_id: self.en_id })
    }
}

/// Encoder context for a short hypothesis window (whisper.cpp pads every
/// input to 30 s; a smaller context makes previews several times faster).
pub fn hypothesis_audio_ctx(samples: usize) -> i32 {
    let secs = samples as f32 / 16_000.0;
    ((secs * 50.0).ceil() as i32 + 64).clamp(256, 1500)
}

/// Below −50 dBFS RMS there is nothing to transcribe.
pub const SILENCE_RMS: f32 = 0.003;

pub fn is_silent(samples: &[f32]) -> bool {
    if samples.is_empty() {
        return true;
    }
    (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt() < SILENCE_RMS
}

impl Stream {
    fn detect(&mut self, samples: &[f32], threads: usize) -> Option<&'static str> {
        self.state.pcm_to_mel(samples, threads).ok()?;
        let (_, probs) = self.state.lang_detect(0, threads).ok()?;
        Some(pick_de_en(&probs, self.de_id, self.en_id))
    }

    pub fn decode(&mut self, samples: &[f32], opts: &DecodeOpts) -> Result<Decoded, String> {
        let threads = opts.threads.max(1);
        if is_silent(samples) {
            // Whisper answers pure silence with "Thank you." — never ask it.
            return Ok(Decoded {
                text: String::new(),
                lang: resolve_lang(opts.lang, None, opts.last_lang.as_deref()),
                confidence: 0.0,
                mean_logprob: 0.0,
                no_speech: 1.0,
                rejected: vec!["silence"],
            });
        }
        let lang = match (opts.lang.forced(), opts.kind) {
            (Some(l), _) => l.to_string(),
            (None, JobKind::Final) => {
                let detected = self.detect(samples, threads);
                resolve_lang(opts.lang, detected, opts.last_lang.as_deref())
            }
            (None, JobKind::Hypothesis) => resolve_lang(opts.lang, None, opts.last_lang.as_deref()),
        };
        let lang_static: &'static str = if lang == "en" { "en" } else { "de" };

        let mut p = match opts.kind {
            JobKind::Final => FullParams::new(SamplingStrategy::BeamSearch { beam_size: 5, patience: -1.0 }),
            JobKind::Hypothesis => FullParams::new(SamplingStrategy::Greedy { best_of: 1 }),
        };
        p.set_n_threads(threads as i32);
        p.set_language(Some(lang_static));
        p.set_translate(false);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        p.set_suppress_blank(true);
        p.set_no_context(true);
        if opts.kind == JobKind::Hypothesis {
            p.set_single_segment(true);
            p.set_audio_ctx(hypothesis_audio_ctx(samples.len()));
            p.set_temperature_inc(0.0);
        }
        let prompt: String = opts.prompt.chars().filter(|c| *c != '\0').collect();
        if !prompt.trim().is_empty() {
            p.set_initial_prompt(&prompt);
        }

        self.state.full(p, samples).map_err(|e| format!("decode: {e}"))?;

        let mut kept = Vec::new();
        let mut probs_all = Vec::new();
        let mut rejected = Vec::new();
        let mut no_speech = 0.0f32;
        for seg in self.state.as_iter() {
            let text = seg.to_str_lossy().map(|c| c.trim().to_string()).unwrap_or_default();
            let mut probs = Vec::new();
            for i in 0..seg.n_tokens() {
                if let Some(t) = seg.get_token(i) {
                    if t.token_id() < self.eot {
                        probs.push(t.token_probability());
                    }
                }
            }
            let lp = mean_logprob(&probs);
            let ns = seg.no_speech_probability();
            no_speech = no_speech.max(ns);
            match accept(&text, lp, ns) {
                Ok(()) => {
                    kept.push(text);
                    probs_all.extend(probs);
                }
                Err(why) => rejected.push(why),
            }
        }
        let kept = drop_loops(kept, 2);
        let mean = mean_logprob(&probs_all);
        Ok(Decoded {
            text: kept.join(" ").split_whitespace().collect::<Vec<_>>().join(" "),
            lang,
            confidence: if probs_all.is_empty() { 0.0 } else { mean.exp() },
            mean_logprob: mean,
            no_speech,
            rejected,
        })
    }
}

/// Word timing on a `load_dtw` engine: greedy (DTW follows one path), token
/// timestamps on; otherwise the final decode's rules — language, no
/// context, prompt, silence never decoded, rejected segments left out.
impl WordTranscriber for Stream {
    fn decode_tokens(&mut self, samples: &[f32], opts: &DecodeOpts) -> Result<TokDecoded, String> {
        let threads = opts.threads.max(1);
        if is_silent(samples) {
            return Ok(TokDecoded { tokens: Vec::new(), lang: resolve_lang(opts.lang, None, opts.last_lang.as_deref()) });
        }
        let lang = match opts.lang.forced() {
            Some(l) => l.to_string(),
            None => {
                let detected = self.detect(samples, threads);
                resolve_lang(opts.lang, detected, opts.last_lang.as_deref())
            }
        };
        let lang_static: &'static str = if lang == "en" { "en" } else { "de" };
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_n_threads(threads as i32);
        p.set_language(Some(lang_static));
        p.set_translate(false);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        p.set_suppress_blank(true);
        p.set_no_context(true);
        p.set_token_timestamps(true);
        let prompt: String = opts.prompt.chars().filter(|c| *c != '\0').collect();
        if !prompt.trim().is_empty() {
            p.set_initial_prompt(&prompt);
        }
        self.state.full(p, samples).map_err(|e| format!("decode: {e}"))?;

        let mut raw = Vec::new();
        for seg in self.state.as_iter() {
            let text = seg.to_str_lossy().map(|c| c.trim().to_string()).unwrap_or_default();
            let mut probs = Vec::new();
            let mut toks = Vec::new();
            for i in 0..seg.n_tokens() {
                let Some(t) = seg.get_token(i) else { continue };
                let special = t.token_id() >= self.eot;
                let d = t.token_data();
                if !special {
                    probs.push(d.p);
                }
                toks.push((t.to_bytes().map(<[u8]>::to_vec).unwrap_or_default(), d.t_dtw, d.p, special));
            }
            if accept(&text, mean_logprob(&probs), seg.no_speech_probability()).is_ok() {
                raw.extend(toks);
            }
        }
        Ok(TokDecoded { tokens: merge_utf8(raw), lang })
    }
}

/// The pipeline's `Transcriber` on top of one shared engine: a whisper
/// state per source.
pub struct WhisperTranscriber {
    engine: std::sync::Arc<Engine>,
    streams: std::collections::HashMap<crate::protocol::Source, Stream>,
}

impl WhisperTranscriber {
    pub fn new(engine: std::sync::Arc<Engine>) -> Self {
        WhisperTranscriber { engine, streams: Default::default() }
    }
}

impl crate::pipeline::Transcriber for WhisperTranscriber {
    fn decode(&mut self, source: crate::protocol::Source, samples: &[f32], opts: &DecodeOpts) -> Result<Decoded, String> {
        if !self.streams.contains_key(&source) {
            let s = self.engine.new_stream()?;
            self.streams.insert(source, s);
        }
        self.streams.get_mut(&source).expect("just inserted").decode(samples, opts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Lang;
    use crate::test_support::fixture;
    use std::path::PathBuf;

    #[test]
    fn silence_is_detected_before_decoding() {
        assert!(is_silent(&[]));
        assert!(is_silent(&[0.001; 1000]));
        assert!(!is_silent(&[0.05; 1000]));
    }

    #[test]
    fn hypothesis_audio_ctx_scales_with_length() {
        assert_eq!(hypothesis_audio_ctx(0), 256);
        assert_eq!(hypothesis_audio_ctx(16_000 * 5), 314);
        assert_eq!(hypothesis_audio_ctx(16_000 * 40), 1500);
    }

    fn model() -> Option<Engine> {
        let p = std::env::var("IR_WHISPER_MODEL_PATH").ok().map(PathBuf::from).unwrap_or_else(|| {
            crate::model::path_of(crate::model::find(crate::model::DEFAULT_MODEL).unwrap())
        });
        p.exists().then(|| Engine::load(&p).unwrap())
    }

    /// What a reload after an idle unload costs: `cargo test --release -p transcribe-core
    /// --lib reload_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the installed model"]
    fn reload_cost() {
        for round in 1..=3 {
            let t = std::time::Instant::now();
            let Some(e) = model() else { return eprintln!("no model") };
            let load = t.elapsed();
            let mut tr = WhisperTranscriber::new(std::sync::Arc::new(e));
            let t = std::time::Instant::now();
            let _ = crate::pipeline::Transcriber::decode(&mut tr, crate::protocol::Source::Mic, &vec![0.01f32; 16_000 * 2], &opts(JobKind::Final, Lang::De));
            eprintln!("round {round}: load {load:?}, first decode {:?}", t.elapsed());
        }
    }

    fn opts(kind: JobKind, lang: Lang) -> DecodeOpts {
        DecodeOpts { kind, lang, last_lang: None, prompt: String::new(), threads: 4 }
    }

    pub fn recall(got: &str, want: &str) -> f32 {
        let g: Vec<String> = crate::text::normalize(got).split(' ').map(str::to_string).collect();
        let w: Vec<String> = crate::text::normalize(want).split(' ').map(str::to_string).collect();
        w.iter().filter(|x| g.contains(x)).count() as f32 / w.len() as f32
    }

    pub const DE: &str = "Wir treffen uns morgen um drei Uhr im Büro und besprechen das neue Projekt.";
    pub const EN: &str = "Let's meet tomorrow at three o'clock in the office and discuss the new project.";

    #[test]
    #[ignore = "needs the whisper model"]
    fn transcribes_german_english_and_picks_the_language() {
        let Some(e) = model() else { return };
        let mut s = e.new_stream().unwrap();
        let de = s.decode(&fixture("de.wav"), &opts(JobKind::Final, Lang::Auto)).unwrap();
        eprintln!("de: {de:?}");
        assert_eq!(de.lang, "de");
        assert!(recall(&de.text, DE) >= 0.9, "{}", de.text);
        let en = s.decode(&fixture("en.wav"), &opts(JobKind::Final, Lang::Auto)).unwrap();
        eprintln!("en: {en:?}");
        assert_eq!(en.lang, "en");
        assert!(recall(&en.text, EN) >= 0.9, "{}", en.text);
    }

    #[test]
    #[ignore = "needs the whisper model"]
    fn silence_yields_nothing() {
        let Some(e) = model() else { return };
        let mut s = e.new_stream().unwrap();
        let d = s.decode(&fixture("silence.wav"), &opts(JobKind::Final, Lang::Auto)).unwrap();
        assert_eq!(d.text, "", "{d:?}");
    }

    #[test]
    #[ignore = "needs the whisper model; prints timings"]
    fn bench_decode() {
        let Some(e) = model() else { return };
        let mut s = e.new_stream().unwrap();
        let de = fixture("de.wav");
        let mut long = Vec::new();
        while long.len() < 16_000 * 15 {
            long.extend_from_slice(&de);
        }
        long.truncate(16_000 * 15);
        let _ = s.decode(&de, &opts(JobKind::Hypothesis, Lang::De)); // warm up
        for (name, kind, n) in [
            ("hyp 5s", JobKind::Hypothesis, 5),
            ("hyp 10s", JobKind::Hypothesis, 10),
            ("final 15s", JobKind::Final, 15),
            ("final 15s auto", JobKind::Final, 15),
        ] {
            let lang = if name.ends_with("auto") { Lang::Auto } else { Lang::De };
            let t = std::time::Instant::now();
            let d = s.decode(&long[..16_000 * n], &opts(kind, lang)).unwrap();
            eprintln!("{name}: {} ms — {} {:?}", t.elapsed().as_millis(), d.text.chars().take(60).collect::<String>(), d.rejected);
        }
    }
}
