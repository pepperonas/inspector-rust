use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use transcribe_protocol::{decode_event, encode, Event};

fn spawn(models: &std::path::Path) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_ir-transcribe"))
        .env("IR_TRANSCRIBE_MODELS_DIR", models)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn next(r: &mut impl BufRead) -> Event {
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    decode_event(&line).unwrap_or_else(|e| panic!("{e}: {line:?}"))
}

#[test]
fn ready_first_errors_keep_it_running_and_eof_ends_it() {
    let models = tempfile::tempdir().unwrap();
    let mut child = spawn(models.path());
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    assert!(matches!(next(&mut out), Event::Ready { .. }));

    writeln!(stdin, "{{\"cmd\":\"nope\"}}").unwrap();
    assert!(matches!(next(&mut out), Event::Error { .. }));

    stdin.write_all(encode(&transcribe_protocol::Command::ListDevices).as_bytes()).unwrap();
    assert!(matches!(next(&mut out), Event::Devices { .. }));

    let cfg = transcribe_protocol::SessionConfig {
        sources: vec![transcribe_protocol::Source::Mic],
        mic_device: None,
        model: "large-v3-turbo-q5".into(),
        lang: transcribe_protocol::Lang::Auto,
        root: models.path().join("t"),
        keep_audio: false,
        dedupe: transcribe_protocol::DedupeMode::Auto,
        dedupe_threshold: 0.75,
        mode: transcribe_protocol::Mode::Meeting,
    };
    stdin.write_all(encode(&transcribe_protocol::Command::Start(cfg)).as_bytes()).unwrap();
    match next(&mut out) {
        Event::Error { error } => assert!(error == "model.missing" || error == "transcribe.unsupported", "{error}"),
        e => panic!("{e:?}"),
    }

    drop(stdin); // the app went away
    let t = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(2), "helper did not exit on EOF");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// End to end with the real model: system audio via the tap, `say` speaks
/// German and English, the transcript holds both lines labelled SYS.
#[test]
#[ignore = "needs the installed model and audio capture permission"]
fn live_session_transcribes_say() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ir-transcribe"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let out = BufReader::new(child.stdout.take().unwrap());
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Event>::new()));
    let ev = events.clone();
    std::thread::spawn(move || {
        for line in out.lines() {
            let e = decode_event(&line.unwrap()).unwrap();
            if !matches!(e, Event::Level { .. }) {
                eprintln!("{e:?}");
            }
            ev.lock().unwrap().push(e);
        }
    });
    let cfg = transcribe_protocol::SessionConfig {
        sources: vec![transcribe_protocol::Source::Sys],
        mic_device: None,
        model: "large-v3-turbo-q5".into(),
        lang: transcribe_protocol::Lang::Auto,
        root: root.path().to_path_buf(),
        keep_audio: true,
        dedupe: transcribe_protocol::DedupeMode::Auto,
        dedupe_threshold: 0.75,
        mode: transcribe_protocol::Mode::Meeting,
    };
    stdin.write_all(encode(&transcribe_protocol::Command::Start(cfg)).as_bytes()).unwrap();
    // Loading the model (Metal) takes a few seconds on the first session.
    let t = Instant::now();
    while !events.lock().unwrap().iter().any(|e| matches!(e, Event::SourceState { state: transcribe_protocol::SourceStatus::Running, .. })) {
        assert!(t.elapsed() < Duration::from_secs(60), "never started");
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("started after {} ms", t.elapsed().as_millis());
    std::thread::sleep(Duration::from_millis(500));
    Command::new("say").args(["-v", "Anna", "Guten Morgen, heute besprechen wir den Zeitplan für das nächste Quartal."]).status().unwrap();
    std::thread::sleep(Duration::from_secs(1));
    Command::new("say").args(["-v", "Samantha", "Thanks everyone, let's start with the budget review."]).status().unwrap();
    std::thread::sleep(Duration::from_secs(4));
    stdin.write_all(encode(&transcribe_protocol::Command::Stop).as_bytes()).unwrap();
    let t = Instant::now();
    while !events.lock().unwrap().iter().any(|e| matches!(e, Event::Stopped { .. })) {
        assert!(t.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(stdin);
    let _ = child.wait();
    let ev = events.lock().unwrap();
    let dir = ev.iter().find_map(|e| match e {
        Event::Stopped { session_dir, .. } => session_dir.clone(),
        _ => None,
    }).unwrap();
    let txt = std::fs::read_to_string(dir.join("transkript.txt")).unwrap();
    eprintln!("---\n{txt}---");
    assert!(txt.contains("SYS:"));
    let lower = txt.to_lowercase();
    assert!(lower.contains("zeitplan"), "{txt}");
    assert!(lower.contains("budget"), "{txt}");
    assert!(ev.iter().any(|e| matches!(e, Event::Hypothesis { .. })), "no live preview");
}

/// File mode end to end: the fixture WAV and an m4a made from it with
/// `afconvert` become sibling TXTs that contain the sentence.
#[test]
#[ignore = "needs the installed model"]
fn file_mode_transcribes_wav_and_m4a() {
    let models = dirs::data_dir().unwrap().join("InspectorRust/models");
    let dir = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../transcribe-core/tests/fixtures/de.wav");
    let wav = dir.path().join("Rede.wav");
    std::fs::copy(&fixture, &wav).unwrap();
    let m4a = dir.path().join("Rede Zwei.m4a");
    let st = Command::new("/usr/bin/afconvert")
        .args(["-f", "m4af", "-d", "aac"])
        .arg(&wav)
        .arg(&m4a)
        .status()
        .unwrap();
    assert!(st.success());

    let mut child = spawn(&models);
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    assert!(matches!(next(&mut out), Event::Ready { .. }));
    for input in [&wav, &m4a] {
        let cmd = transcribe_protocol::Command::TranscribeFile {
            path: input.clone(),
            lang: transcribe_protocol::Lang::Auto,
            model: "large-v3-turbo-q5".into(),
        };
        stdin.write_all(encode(&cmd).as_bytes()).unwrap();
        let mut progressed = false;
        let txt = loop {
            match next(&mut out) {
                Event::FileProgress { .. } => progressed = true,
                Event::FileDone { txt, lang } => {
                    assert_eq!(lang.as_deref(), Some("de"));
                    break txt;
                }
                Event::FileError { error } => panic!("{error}"),
                _ => {}
            }
        };
        assert!(progressed);
        let text = std::fs::read_to_string(&txt).unwrap();
        eprintln!("{} → {text}", input.display());
        let low = text.to_lowercase();
        assert!(low.contains("morgen") && low.contains("projekt"), "{text}");
    }
    drop(stdin);
    let _ = child.wait();
}

/// Word timing end to end: `say` speaks a known German sentence into a WAV,
/// `TranscribeWords` returns its words in order with monotonic times that end
/// inside the file.
#[test]
#[ignore = "needs the installed model"]
fn file_words() {
    let models = dirs::data_dir().unwrap().join("InspectorRust/models");
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("satz.wav");
    let sentence = "Guten Morgen, heute besprechen wir den Zeitplan für das nächste Quartal.";
    let st = Command::new("say")
        .args(["-v", "Anna", "--data-format=LEI16@16000", "-o"])
        .arg(&wav)
        .arg(sentence)
        .status()
        .unwrap();
    assert!(st.success());
    let bytes = std::fs::metadata(&wav).unwrap().len();
    let file_ms = bytes.saturating_sub(44) * 1000 / (16_000 * 2);

    let mut child = spawn(&models);
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    assert!(matches!(next(&mut out), Event::Ready { .. }));
    let cmd = transcribe_protocol::Command::TranscribeWords {
        path: wav.clone(),
        lang: transcribe_protocol::Lang::De,
        model: "large-v3-turbo-q5".into(),
    };
    let t = Instant::now();
    stdin.write_all(encode(&cmd).as_bytes()).unwrap();
    let (words, lang) = loop {
        match next(&mut out) {
            Event::FileWords { words, lang } => break (words, lang),
            Event::FileError { error } => panic!("{error}"),
            _ => {}
        }
    };
    eprintln!("{} ms, file {file_ms} ms, lang {lang:?}", t.elapsed().as_millis());
    for w in &words {
        eprintln!("  {:>6}–{:>6} {:.2} {}", w.start_ms, w.end_ms, w.p, w.text);
    }
    let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let got: Vec<String> = words.iter().map(|w| norm(&w.text)).collect();
    let want = ["guten", "morgen", "heute", "besprechen", "wir", "den", "zeitplan", "für", "das", "nächste", "quartal"];
    let mut it = got.iter();
    for w in want {
        assert!(it.any(|g| g == w), "{w} missing or out of order in {got:?}");
    }
    let mut prev = 0;
    for w in &words {
        assert!(w.start_ms >= prev && w.end_ms >= w.start_ms, "not monotonic: {words:?}");
        prev = w.end_ms;
    }
    assert!(prev <= file_ms + 200, "last word ends at {prev}, file is {file_ms} ms");
    drop(stdin);
    let _ = child.wait();
}
