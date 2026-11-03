use super::*;
use std::path::PathBuf;

fn cfg() -> SessionConfig {
    SessionConfig {
        sources: vec![Source::Mic, Source::Sys],
        mic_device: Some("MacBook Pro-Mikrofon".into()),
        model: "large-v3-turbo-q5".into(),
        lang: Lang::Auto,
        root: PathBuf::from("/tmp/x"),
        keep_audio: true,
        dedupe: DedupeMode::Auto,
        dedupe_threshold: 0.75,
        mode: Mode::Meeting,
    }
}

fn commands() -> Vec<Command> {
    vec![
        Command::Start(cfg()),
        Command::Stop,
        Command::SetSource { source: Source::Sys, on: false },
        Command::Window { source: Source::Mic, secs: 10 },
        Command::DownloadModel { model: "small-q5".into() },
        Command::CancelDownload,
        Command::TranscribeFile { path: "/a b.m4a".into(), lang: Lang::De, model: "x".into() },
        Command::CancelFile,
        Command::ListDevices,
        Command::Shutdown,
    ]
}

fn events() -> Vec<Event> {
    vec![
        Event::Ready { version: "1".into() },
        Event::Started { session_dir: "/s".into(), sources: vec![Source::Mic] },
        Event::Hypothesis { source: Source::Mic, seq: 3, stable: "Hallo".into(), pending: "Welt\nzwei".into() },
        Event::Final {
            source: Source::Sys,
            seq: 4,
            start_ms: 1,
            end_ms: 2,
            lang: "de".into(),
            text: "Zeile eins\nZeile zwei".into(),
            confidence: 0.9,
        },
        Event::Dropped { source: Source::Mic, seq: 5, reason: "echo".into() },
        Event::Level { source: Source::Sys, rms: 0.1 },
        Event::SourceState { source: Source::Sys, state: SourceStatus::Lost, detail: Some("permission".into()) },
        Event::Warning { code: "w".into(), detail: "d".into() },
        Event::Stopped { session_dir: None, error: Some("e".into()) },
        Event::Window { source: Source::Mic, pcm_b64: "AA==".into() },
        Event::ModelProgress { model: "m".into(), done: 1, total: 2 },
        Event::ModelReady { model: "m".into() },
        Event::ModelError { model: "m".into(), error: "e".into() },
        Event::FileProgress { done_ms: 1, total_ms: 2 },
        Event::FileDone { txt: "/x.txt".into() },
        Event::FileError { error: "e".into() },
        Event::Devices { mic: vec!["a".into()] },
        Event::Error { error: "e".into() },
    ]
}

#[test]
fn roundtrip_every_command_and_event() {
    for c in commands() {
        let line = encode(&c);
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1, "{line}");
        assert_eq!(decode_command(&line).unwrap(), c);
    }
    for e in events() {
        let line = encode(&e);
        assert_eq!(line.matches('\n').count(), 1, "{line}");
        assert_eq!(decode_event(&line).unwrap(), e);
    }
}

#[test]
fn unknown_command_is_an_error_not_a_panic() {
    assert!(decode_command("{\"cmd\":\"nope\"}").is_err());
    assert!(decode_command("not json").is_err());
    assert!(decode_event("{\"event\":\"nope\"}").is_err());
}

#[test]
fn source_labels() {
    assert_eq!(Source::Mic.label(), "MIC");
    assert_eq!(Source::Sys.label(), "SYS");
    assert_eq!(encode(&Source::Mic).trim(), "\"mic\"");
}
