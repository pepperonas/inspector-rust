use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Text,
    Rtf,
    Html,
    Image,
    Files,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentType::Text => "text",
            ContentType::Rtf => "rtf",
            ContentType::Html => "html",
            ContentType::Image => "image",
            ContentType::Files => "files",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "text" => Some(ContentType::Text),
            "rtf" => Some(ContentType::Rtf),
            "html" => Some(ContentType::Html),
            "image" => Some(ContentType::Image),
            "files" => Some(ContentType::Files),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipEntry {
    pub id: i64,
    pub content_type: ContentType,
    /// Plain-text preview (always populated for search).
    pub content_text: String,
    /// For text/rtf/html: the raw payload string.
    /// For image: base64-encoded PNG.
    /// For files: JSON array of paths.
    pub content_data: String,
    pub hash: String,
    pub byte_size: i64,
    pub created_at: i64,
    pub last_used_at: i64,
}

/// Payload coming in from the clipboard watcher, not yet hashed/stored.
#[derive(Debug, Clone)]
pub struct NewClip {
    pub content_type: ContentType,
    pub content_text: String,
    pub content_data: String,
    pub byte_size: i64,
}

/// 5 MB per-entry ceiling for images.
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

/// History is pruned to this many most-recently-used entries.
pub const MAX_ENTRIES: i64 = 1000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_as_str_round_trips() {
        let pairs = [
            (ContentType::Text, "text"),
            (ContentType::Rtf, "rtf"),
            (ContentType::Html, "html"),
            (ContentType::Image, "image"),
            (ContentType::Files, "files"),
        ];
        for (ct, s) in pairs {
            assert_eq!(ct.as_str(), s, "as_str mismatch for {ct:?}");
            assert_eq!(ContentType::from_str(s), Some(ct), "from_str mismatch for {s}");
        }
    }

    #[test]
    fn from_str_returns_none_for_unknown_input() {
        assert_eq!(ContentType::from_str("unknown"), None);
        assert_eq!(ContentType::from_str("TEXT"), None);
        assert_eq!(ContentType::from_str(""), None);
        assert_eq!(ContentType::from_str(" text"), None);
    }

    #[test]
    fn content_type_is_copy() {
        let ct = ContentType::Text;
        let _ct2 = ct;
        let _ct3 = ct; // Would fail to compile if not Copy
    }

    #[test]
    fn content_type_serde_uses_lowercase_strings() {
        // Frontend TS types expect lowercase enum tags.
        // `#[serde(rename_all = "lowercase")]` is load-bearing.
        let json = serde_json::to_string(&ContentType::Image).unwrap();
        assert_eq!(json, r#""image""#);
        let back: ContentType = serde_json::from_str(r#""html""#).unwrap();
        assert_eq!(back, ContentType::Html);
    }

    #[test]
    fn content_type_serde_rejects_uppercase() {
        let r: Result<ContentType, _> = serde_json::from_str(r#""IMAGE""#);
        assert!(r.is_err(), "uppercase 'IMAGE' should not deserialise");
    }

    #[test]
    fn max_image_bytes_is_five_megabytes() {
        assert_eq!(MAX_IMAGE_BYTES, 5 * 1024 * 1024);
    }

    #[test]
    fn max_entries_history_cap_is_one_thousand() {
        assert_eq!(MAX_ENTRIES, 1000);
    }

    #[test]
    fn clip_entry_serde_round_trip() {
        let original = ClipEntry {
            id: 42,
            content_type: ContentType::Files,
            content_text: "/tmp/a.txt\n/tmp/b.txt".into(),
            content_data: r#"["/tmp/a.txt","/tmp/b.txt"]"#.into(),
            hash: "abc123".into(),
            byte_size: 23,
            created_at: 1_700_000_000_000,
            last_used_at: 1_700_000_000_500,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: ClipEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, original.id);
        assert_eq!(back.content_type, original.content_type);
        assert_eq!(back.content_text, original.content_text);
        assert_eq!(back.content_data, original.content_data);
        assert_eq!(back.hash, original.hash);
        assert_eq!(back.byte_size, original.byte_size);
        assert_eq!(back.created_at, original.created_at);
        assert_eq!(back.last_used_at, original.last_used_at);
    }

    #[test]
    fn new_clip_is_clone() {
        let c = NewClip {
            content_type: ContentType::Html,
            content_text: "<p>hi</p>".into(),
            content_data: "<p>hi</p>".into(),
            byte_size: 9,
        };
        let c2 = c.clone();
        assert_eq!(c.content_text, c2.content_text);
    }

    #[test]
    fn content_type_equality_excludes_other_variants() {
        assert_ne!(ContentType::Text, ContentType::Rtf);
        assert_ne!(ContentType::Html, ContentType::Image);
        assert_ne!(ContentType::Image, ContentType::Files);
    }
}
