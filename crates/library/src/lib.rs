//! clipper23 library: SQLite-backed index of saved clips plus file management.

/// A single clip as stored in the library.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Clip {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub size_bytes: u64,
    pub created_at: i64,
    pub thumbnail_path: Option<String>,
    pub favorite: bool,
}
