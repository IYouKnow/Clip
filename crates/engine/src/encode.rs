//! Encoder selection and probing.

/// H.264 encoders in preference order.
///
/// NVENC (NVIDIA) and AMF (AMD) are dedicated hardware; `h264_mf` uses Media
/// Foundation and covers Intel plus any machine without the vendor encoders.
/// `libx264` is deliberately last: the vendored FFmpeg is LGPL and does not
/// ship it, but a custom GPL build would be picked up automatically.
pub const H264_ENCODERS: &[&str] = &["h264_nvenc", "h264_amf", "h264_qsv", "h264_mf", "libx264"];

/// Availability of one candidate encoder.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EncoderInfo {
    pub name: String,
    pub available: bool,
}

/// Reports which known H.264 encoders this FFmpeg build exposes.
pub fn h264_encoders() -> Vec<EncoderInfo> {
    H264_ENCODERS
        .iter()
        .map(|name| EncoderInfo {
            name: (*name).to_string(),
            available: ffmpeg_next::encoder::find_by_name(name).is_some(),
        })
        .collect()
}

/// Picks the best available H.264 encoder for this machine.
pub fn pick_h264_encoder() -> Option<&'static str> {
    H264_ENCODERS
        .iter()
        .copied()
        .find(|name| ffmpeg_next::encoder::find_by_name(name).is_some())
}
