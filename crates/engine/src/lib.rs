//! clipper23 engine: capture, encode, replay buffer and clip saving.
//!
//! Modules are added as the pipeline is built:
//! `capture` -> `convert` -> `encode` -> `ring` (+ `audio`) -> `mux` -> `session`.

pub mod capture;
pub mod encode;
pub mod mux;
pub mod ring;
pub mod session;

/// Current engine status, surfaced to the UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineStatus {
    pub replaying: bool,
    pub encoder: Option<String>,
    pub buffer_seconds: u32,
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self {
            replaying: false,
            encoder: None,
            buffer_seconds: 60,
        }
    }
}
