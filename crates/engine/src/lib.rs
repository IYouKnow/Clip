//! trace engine: capture, encode, replay buffer and clip saving.
//!
//! Pipeline: `capture` -> `encode` -> `ring` (+ `audio`) -> `mux` -> `session`.

pub mod audio;
pub mod audio_encode;
pub mod capture;
pub mod encode;
pub mod hw;
pub mod mux;
pub mod ring;
pub mod session;
