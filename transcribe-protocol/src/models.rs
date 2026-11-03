//! The model catalogue and where models live — shared by the app (status,
//! sizes) and the helper (download, loading).

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ModelInfo {
    pub id: &'static str,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

pub const DEFAULT_MODEL: &str = "large-v3-turbo-q5";

pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: "large-v3-turbo-q5",
        file: "ggml-large-v3-turbo-q5_0.bin",
        bytes: 574_041_195,
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
    },
    ModelInfo {
        id: "large-v3-turbo-q8",
        file: "ggml-large-v3-turbo-q8_0.bin",
        bytes: 874_188_075,
        sha256: "317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1",
    },
    ModelInfo {
        id: "medium-q5",
        file: "ggml-medium-q5_0.bin",
        bytes: 539_212_467,
        sha256: "19fea4b380c3a618ec4723c3eef2eb785ffba0d0538cf43f8f235e7b3b34220f",
    },
    ModelInfo {
        id: "small-q5",
        file: "ggml-small-q5_1.bin",
        bytes: 190_085_487,
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
    },
];

pub const ERR_SHA: &str = "model.sha_mismatch";
pub const ERR_CANCELLED: &str = "model.cancelled";
pub const ERR_UNKNOWN: &str = "model.unknown";
pub const ERR_MISSING: &str = "model.missing";

pub fn find(id: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.id == id)
}

/// `IR_TRANSCRIBE_MODELS_DIR` overrides the folder (tests, diagnostics).
pub fn models_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("IR_TRANSCRIBE_MODELS_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("InspectorRust")
        .join("models")
}

pub fn path_of(m: &ModelInfo) -> PathBuf {
    models_dir().join(m.file)
}

/// Installed = present with the catalogue size. A full hash on every start
/// would read half a gigabyte; the hash is checked once, when downloading.
pub fn is_installed(m: &ModelInfo) -> bool {
    is_installed_at(&path_of(m), m)
}

pub fn is_installed_at(path: &Path, m: &ModelInfo) -> bool {
    fs::metadata(path).is_ok_and(|md| md.is_file() && md.len() == m.bytes)
}

