//! The decode request/result types shared by the engine and the pipeline
//! (kept out of `engine` so the pipeline builds and tests without whisper.cpp).

use crate::protocol::Lang;
use crate::queue::JobKind;

#[derive(Debug, Clone)]
pub struct DecodeOpts {
    pub kind: JobKind,
    pub lang: Lang,
    /// Language of the source's last final, used for hypotheses in auto mode.
    pub last_lang: Option<String>,
    pub prompt: String,
    pub threads: usize,
    /// Decode a preview like a final (beam search, full encoder context).
    /// Music needs it: the fast preview settings run into repeat loops there
    /// and nearly every preview is thrown away.
    pub careful: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub text: String,
    pub lang: String,
    /// `exp(mean_logprob)` of the kept segments, 0..1.
    pub confidence: f32,
    pub mean_logprob: f32,
    pub no_speech: f32,
    /// Why segments were left out (diagnostics).
    pub rejected: Vec<&'static str>,
}

