//! Diagnostic: does video-processor output-view creation work on each adapter?
//!
//! The default adapter on a remote/virtualised session can be a display-only
//! adapter with no video processor, which rejects output views while still
//! allowing capture.
//!
//! Usage: `cargo run -p trace-engine --example adapter_probe`

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPOV_DIMENSION_TEXTURE2D, D3D11CreateDevice,
    ID3D11Device, ID3D11Texture2D, ID3D11VideoDevice, ID3D11VideoProcessorOutputView,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1};

fn test(device: &ID3D11Device, label: &str) {
    let Ok(video) = device.cast::<ID3D11VideoDevice>() else {
        println!("[{label}] no video device");
        return;
    };
    let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
        InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        InputFrameRate: DXGI_RATIONAL {
            Numerator: 60,
            Denominator: 1,
        },
        InputWidth: 1920,
        InputHeight: 1080,
        OutputFrameRate: DXGI_RATIONAL {
            Numerator: 60,
            Denominator: 1,
        },
        OutputWidth: 1920,
        OutputHeight: 1080,
        Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
    };
    let Ok(enumerator) = (unsafe { video.CreateVideoProcessorEnumerator(&desc) }) else {
        println!("[{label}] no enumerator");
        return;
    };
    if unsafe { video.CreateVideoProcessor(&enumerator, 0) }.is_err() {
        println!("[{label}] no processor");
        return;
    }

    let tex_desc = D3D11_TEXTURE2D_DESC {
        Width: 1920,
        Height: 1080,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: 0x8,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut tex: Option<ID3D11Texture2D> = None;
    if let Err(error) = unsafe { device.CreateTexture2D(&tex_desc, None, Some(&mut tex)) } {
        println!("[{label}] no NV12 texture: {error}");
        return;
    }
    let tex = tex.unwrap();

    let view_desc = {
        let mut d = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC::default();
        d.ViewDimension = D3D11_VPOV_DIMENSION_TEXTURE2D;
        d.Anonymous.Texture2D = D3D11_TEX2D_VPOV { MipSlice: 0 };
        d
    };
    let mut view: Option<ID3D11VideoProcessorOutputView> = None;
    let result =
        unsafe { video.CreateVideoProcessorOutputView(&tex, &enumerator, &view_desc, Some(&mut view)) };
    println!("[{label}] output view: {result:?}");

    // Variations that occasionally matter per driver.
    let variations: &[(&str, u32, u32, i32)] = &[
        ("NV12 shared", 0x8, 0x2, DXGI_FORMAT_NV12.0),
        ("BGRA rt|srv", 0x8 | 0x4, 0, 87),
        ("BGRA rt", 0x8, 0, 87),
    ];
    for (name, bind, misc, format) in variations {
        let mut d = tex_desc;
        d.BindFlags = *bind;
        d.MiscFlags = *misc;
        d.Format = windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT(*format);
        let mut t: Option<ID3D11Texture2D> = None;
        if let Err(error) = unsafe { device.CreateTexture2D(&d, None, Some(&mut t)) } {
            println!("[{label}] {name}: texture failed: {error}");
            continue;
        }
        let t = t.unwrap();
        let mut view: Option<ID3D11VideoProcessorOutputView> = None;
        let result =
            unsafe { video.CreateVideoProcessorOutputView(&t, &enumerator, &view_desc, Some(&mut view)) };
        println!("[{label}] {name}: {result:?}");
    }
}

fn create(adapter: Option<&IDXGIAdapter1>) -> Option<ID3D11Device> {
    let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
    let mut device: Option<ID3D11Device> = None;
    let mut level = D3D_FEATURE_LEVEL::default();
    let driver = if adapter.is_some() {
        D3D_DRIVER_TYPE_UNKNOWN
    } else {
        D3D_DRIVER_TYPE_HARDWARE
    };
    let base: Option<IDXGIAdapter> = adapter.and_then(|a| a.cast().ok());
    let adapter = base.as_ref();
    let result = unsafe {
        D3D11CreateDevice(
            adapter,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut level),
            None,
        )
    };
    match result {
        Ok(()) => device,
        Err(error) => {
            println!("    device creation failed: {error}");
            None
        }
    }
}

fn main() -> anyhow::Result<()> {
    if let Some(device) = create(None) {
        test(&device, "default adapter");
    }

    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    let mut index = 0;
    loop {
        let adapter: Result<IDXGIAdapter1, _> = unsafe { factory.EnumAdapters1(index) };
        let Ok(adapter) = adapter else { break };
        let desc = unsafe { adapter.GetDesc() }.unwrap_or_default();
        let name = String::from_utf16_lossy(
            &desc.Description[..desc
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(desc.Description.len())],
        );
        println!("adapter {index}: {name}");
        if let Some(device) = create(Some(&adapter)) {
            test(&device, &name);
        }
        index += 1;
    }

    Ok(())
}
