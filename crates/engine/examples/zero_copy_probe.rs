//! Minimal repro of the GPU (D3D11VA) encode path teardown. Builds an NV12
//! texture, optional video processor, and FFmpeg hardware frames, then drops
//! everything with logging so a crash can be localized.
//!
//! Env: `ZC_BIND=<hex>` bind flags, `ZC_SKIP_PROCESSOR`, `ZC_SKIP_ENCODER`.
//!
//! Usage: `cargo run -p trace-engine --example zero_copy_probe`

use std::sync::Arc;

use ffmpeg_next::ffi::AVPixelFormat;
use trace_engine::encode::VideoEncoder;
use trace_engine::hw::{self, GpuDevice, HwFrames, VideoProcessor};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const FPS: u32 = 60;

fn env_flag(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

fn main() -> anyhow::Result<()> {
    let feature_levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
    let mut device = None;
    let mut feature_level = D3D_FEATURE_LEVEL::default();
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut feature_level),
            Some(&mut context),
        )?;
    }
    let gpu = GpuDevice::from_parts(device.unwrap(), context.unwrap());

    let bind = std::env::var("ZC_BIND")
        .ok()
        .and_then(|value| u32::from_str_radix(value.trim_start_matches("0x"), 16).ok())
        .unwrap_or(hw::BIND_RENDER_TARGET);
    println!("bind flags: 0x{bind:02x}");

    let (nv12, slices) = hw::create_nv12_texture(&gpu, WIDTH, HEIGHT, hw::FRAME_POOL_SIZE, bind)?;
    println!("nv12 texture: {slices} slices");

    let processor = if env_flag("ZC_SKIP_PROCESSOR") {
        None
    } else {
        Some(VideoProcessor::new(&gpu, &nv12, slices, WIDTH, HEIGHT)?)
    };
    println!("processor: {}", if processor.is_some() { "built" } else { "skipped" });

    let hw_frames = HwFrames::from_texture(
        &gpu,
        nv12,
        WIDTH,
        HEIGHT,
        slices,
        AVPixelFormat::AV_PIX_FMT_NV12,
        bind,
    )?;
    println!("hw frames built");
    let hw_frames = Arc::new(hw_frames);

    if env_flag("ZC_SKIP_ENCODER") {
        println!("dropping processor");
        drop(processor);
        println!("dropping hw_frames");
        drop(hw_frames);
        println!("clean teardown (no encoder)");
        return Ok(());
    }

    let (bgra, _) = hw::create_texture(&gpu, WIDTH, HEIGHT, 1, DXGI_FORMAT_B8G8R8A8_UNORM, bind)?;
    let processor = processor.expect("processor needed for encoding");
    let mut encoder =
        VideoEncoder::new_with_hw(None, WIDTH, HEIGHT, FPS, 20_000_000, Some(hw_frames.clone()), true)?;
    println!("encoder opened: {} ({:?})", encoder.name(), encoder.kind());

    for index in 0..5 {
        let mut frame = hw_frames.get_frame()?;
        unsafe { processor.convert(&bgra, frame.slice_index())? };
        let pts = index as i64 * 1_000_000 / FPS as i64;
        frame.set_pts(pts);
        let packets = encoder.encode_av_frame(frame.into_raw(), pts)?;
        println!("frame {index}: {} packets", packets.len());
    }
    println!("flushed: {} packets", encoder.flush()?.len());

    println!("dropping encoder");
    drop(encoder);
    println!("dropping processor");
    drop(processor);
    println!("dropping hw_frames");
    drop(hw_frames);
    println!("dropping bgra");
    drop(bgra);
    println!("clean teardown");
    Ok(())
}
