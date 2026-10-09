//! Diagnostic: which input pixel formats will each H.264 encoder actually open with?
//!
//! Usage: `cargo run -p trace-engine --example encoder_formats`

use ffmpeg_next as ffmpeg;
use ffmpeg::format::Pixel;

fn main() {
    let _ = ffmpeg::init();

    let encoders = ["h264_nvenc", "h264_amf", "h264_mf", "libopenh264"];
    let formats = [
        ("nv12", Pixel::NV12),
        ("yuv420p", Pixel::YUV420P),
        ("bgra", Pixel::BGRA),
        ("rgba", Pixel::RGBA),
        ("yuv444p", Pixel::YUV444P),
    ];

    for name in encoders {
        let Some(codec) = ffmpeg::encoder::find_by_name(name) else {
            println!("{name}: not compiled in");
            continue;
        };
        for (label, format) in formats {
            let mut context = ffmpeg::codec::context::Context::new_with_codec(codec);
            context.set_time_base(ffmpeg::Rational(1, 1_000_000));
            context.set_frame_rate(Some(ffmpeg::Rational(60, 1)));
            let Ok(mut video) = context.encoder().video() else {
                println!("{name} {label}: no video encoder");
                continue;
            };
            video.set_width(1920);
            video.set_height(1080);
            video.set_bit_rate(20_000_000);
            video.set_format(format);
            video.set_gop(120);
            video.set_max_b_frames(0);
            match video.open() {
                Ok(_) => println!("{name} {label}: OK"),
                Err(error) => println!("{name} {label}: {error}"),
            }
        }
    }
}
