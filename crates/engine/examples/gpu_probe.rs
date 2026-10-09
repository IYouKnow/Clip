//! Diagnostic: builds the NV12 texture + D3D11 video-processor output view for
//! each candidate bind-flag set and reports which the driver accepts, so the
//! zero-copy capture path can be checked without the UI.
//!
//! Usage: `cargo run -p trace-engine --example gpu_probe`

use trace_engine::hw::{self, GpuDevice, VideoProcessor};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;

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
    let device = device.expect("D3D11CreateDevice returned no device");
    let context = context.expect("D3D11CreateDevice returned no context");
    println!("created D3D11 device (feature level {feature_level:?}), {WIDTH}x{HEIGHT}");

    let gpu = GpuDevice::from_parts(device, context);

    let mut attempts: Vec<(u32, u32)> = hw::BIND_CANDIDATES
        .iter()
        .map(|flags| (*flags, hw::FRAME_POOL_SIZE))
        .collect();
    attempts.push((hw::BIND_RENDER_TARGET, 1));

    let mut any_ok = false;
    for (flags, pool) in attempts {
        match build(&gpu, flags, pool) {
            Ok(slices) => {
                any_ok = true;
                println!(
                    "flags=0x{flags:02x} pool={pool} -> OK (texture slices={slices}, processor + output view built)"
                );
            }
            Err(error) => {
                println!("flags=0x{flags:02x} pool={pool} -> FAILED: {error:#}");
            }
        }
    }

    if any_ok {
        println!("RESULT: GPU zero-copy path is available");
    } else {
        println!("RESULT: no GPU combination worked; CPU capture fallback is expected");
    }
    Ok(())
}

/// Attempts the texture + processor + output view for one candidate.
fn build(gpu: &GpuDevice, bind_flags: u32, pool: u32) -> anyhow::Result<u32> {
    let (texture, slices) = hw::create_nv12_texture(gpu, WIDTH, HEIGHT, pool, bind_flags)?;
    let _processor = VideoProcessor::new(gpu, &texture, slices, WIDTH, HEIGHT)?;
    Ok(slices)
}
