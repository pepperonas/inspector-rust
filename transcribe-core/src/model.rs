//! Model catalogue, verified download and the embedded VAD model.
//!
//! Models live in `~/Library/Application Support/InspectorRust/models/`. A
//! download streams into `<file>.part`, hashes on the fly and is renamed only
//! when the SHA-256 matches the catalogue; a mismatch deletes the partial file.

pub use crate::protocol::models::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use std::sync::atomic::{AtomicBool, Ordering};

const BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";

/// Silero VAD v6.2.0 in ggml form (MIT, ggml-org/whisper-vad).
const VAD_MODEL: &[u8] = include_bytes!("../models/ggml-silero-v6.2.0.bin");
pub const VAD_FILE: &str = "ggml-silero-v6.2.0.bin";
pub const VAD_BYTES: u64 = 885_098;
pub const VAD_SHA256: &str = "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987";


fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Download `m` into the models folder.
pub fn download(m: &ModelInfo, cancel: &AtomicBool, progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    download_from(&format!("{BASE_URL}{}", m.file), m, &models_dir(), cancel, progress)
}

pub fn download_from(
    url: &str,
    m: &ModelInfo,
    dir: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let target = dir.join(m.file);
    if is_installed_at(&target, m) {
        return Ok(target);
    }
    let part = dir.join(format!("{}.part", m.file));
    let result = (|| {
        let resp = ureq::get(url).call().map_err(|e| format!("download: {e}"))?;
        let mut reader = resp.into_reader();
        let mut out = fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        let mut done = 0u64;
        let mut last_report = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ERR_CANCELLED.to_string());
            }
            let n = reader.read(&mut buf).map_err(|e| format!("download: {e}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| format!("write: {e}"))?;
            hasher.update(&buf[..n]);
            done += n as u64;
            if done - last_report >= 1 << 20 {
                last_report = done;
                progress(done, m.bytes);
            }
        }
        out.sync_all().map_err(|e| format!("write: {e}"))?;
        progress(done, m.bytes);
        if hex(&hasher.finalize()) != m.sha256 {
            return Err(ERR_SHA.to_string());
        }
        fs::rename(&part, &target).map_err(|e| format!("rename: {e}"))?;
        Ok(target.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

/// Path of the VAD model, written from the embedded copy on first use.
pub fn vad_model_path() -> Result<PathBuf, String> {
    vad_model_path_in(&models_dir())
}

pub fn vad_model_path_in(dir: &Path) -> Result<PathBuf, String> {
    // Both sources of a session ask at the same moment; one writes, the other
    // waits and then finds the finished file.
    static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let p = dir.join(VAD_FILE);
    if fs::metadata(&p).is_ok_and(|md| md.len() == VAD_BYTES) {
        return Ok(p);
    }
    if VAD_MODEL.len() as u64 != VAD_BYTES || sha256_hex(VAD_MODEL) != VAD_SHA256 {
        return Err(ERR_SHA.to_string());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // A unique name: another process (a second helper) can't collide either.
    let tmp = dir.join(format!("{VAD_FILE}.{}.tmp", std::process::id()));
    fs::write(&tmp, VAD_MODEL).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A one-shot HTTP server that answers every request with `body`.
    fn serve(body: Vec<u8>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                let mut req = [0u8; 1024];
                let _ = s.read(&mut req);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            }
        });
        format!("http://{addr}/m.bin")
    }

    fn leak_info(body: &[u8], sha: String) -> ModelInfo {
        ModelInfo { id: "t", file: "t.bin", bytes: body.len() as u64, sha256: Box::leak(sha.into_boxed_str()) }
    }

    #[test]
    fn catalogue_entries_are_unique_and_have_64_hex_sha() {
        assert!(find(DEFAULT_MODEL).is_some());
        let mut ids: Vec<_> = MODELS.iter().map(|m| m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), MODELS.len());
        for m in MODELS {
            assert_eq!(m.sha256.len(), 64);
            assert!(m.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(m.bytes > 100_000_000);
        }
    }

    #[test]
    fn download_verifies_sha() {
        let body = vec![7u8; 3_000_000];
        let info = leak_info(&body, sha256_hex(&body));
        let dir = tempfile::tempdir().unwrap();
        let mut calls = 0;
        let p = download_from(&serve(body), &info, dir.path(), &AtomicBool::new(false), |_, _| calls += 1).unwrap();
        assert_eq!(fs::metadata(&p).unwrap().len(), 3_000_000);
        assert!(!dir.path().join("t.bin.part").exists());
        assert!(calls >= 2);
    }

    #[test]
    fn wrong_sha_removes_partial() {
        let body = vec![1u8; 10_000];
        let info = leak_info(&body, "0".repeat(64));
        let dir = tempfile::tempdir().unwrap();
        let r = download_from(&serve(body), &info, dir.path(), &AtomicBool::new(false), |_, _| {});
        assert_eq!(r, Err(ERR_SHA.to_string()));
        assert!(!dir.path().join("t.bin.part").exists());
        assert!(!dir.path().join("t.bin").exists());
    }

    #[test]
    fn cancel_stops_download() {
        let body = vec![1u8; 10_000];
        let info = leak_info(&body, sha256_hex(&body));
        let dir = tempfile::tempdir().unwrap();
        let r = download_from(&serve(body), &info, dir.path(), &AtomicBool::new(true), |_, _| {});
        assert_eq!(r, Err(ERR_CANCELLED.to_string()));
        assert!(!dir.path().join("t.bin").exists());
    }

    #[test]
    fn embedded_vad_model_matches_its_hash() {
        assert_eq!(VAD_MODEL.len() as u64, VAD_BYTES);
        assert_eq!(sha256_hex(VAD_MODEL), VAD_SHA256);
        let dir = tempfile::tempdir().unwrap();
        let p = vad_model_path_in(dir.path()).unwrap();
        assert_eq!(fs::read(p).unwrap(), VAD_MODEL);
    }

    #[test]
    fn two_sources_asking_at_once_both_get_the_model() {
        for _ in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let d = dir.path().to_path_buf();
            let hs: Vec<_> = (0..4)
                .map(|_| {
                    let d = d.clone();
                    std::thread::spawn(move || vad_model_path_in(&d))
                })
                .collect();
            for h in hs {
                let p = h.join().unwrap().expect("every caller gets the path");
                assert_eq!(fs::metadata(&p).unwrap().len(), VAD_BYTES);
            }
            let left: Vec<_> = fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
            assert_eq!(left.len(), 1, "no temp files left: {left:?}");
        }
    }
}
