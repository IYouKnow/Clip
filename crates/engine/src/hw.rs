//! GPU-side plumbing for a zero-copy capture → encode path.
//!
//! The capture device (owned by `windows-capture`) is shared with FFmpeg through
//! a D3D11VA hardware-device context, so captured frames never have to make the
//! GPU → CPU → GPU round trip. A D3D11 video processor converts the captured
//! BGRA texture into an NV12 texture that lives in an FFmpeg hardware frame pool.
//!
//! All `unsafe` and FFI lives here; the rest of the engine uses the safe types
//! below. If any step fails to initialise (no video device, no NV12 support,
//! no hardware encoder) the caller falls back to the CPU path.

use std::mem::ManuallyDrop;
use std::ptr;

use anyhow::{anyhow, Result};
use ffmpeg_next::ffi;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_TEX2D_ARRAY_VPOV, D3D11_TEX2D_VPIV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2DARRAY, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC};
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
};

/// `D3D11_BIND_RENDER_TARGET`.
///
/// The capture device only accepts NV12 textures with a render-target bind flag;
/// `D3D11_BIND_VIDEO_ENCODER`, `DECODER` and `SHADER_RESOURCE` all return
/// E_INVALIDARG for NV12 here, and hardware encoders accept a render-target
/// texture as input.
const ENCODER_TEXTURE_BIND_FLAGS: u32 = 0x0000_0008;

/// Number of NV12 frames kept in the hardware pool.
pub const FRAME_POOL_SIZE: u32 = 8;

fn hr(code: i32) -> anyhow::Error {
    anyhow!("hardware call failed (0x{:08x})", code as u32)
}

/// Lowers the calling thread's priority so capture/encoding never outrank the
/// rest of the machine.
pub fn set_current_thread_below_normal() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

/// The D3D11 device/context that capture and encoding share.
#[derive(Clone)]
pub struct GpuDevice {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
}

impl GpuDevice {
    /// Wraps an existing device/context (the ones `windows-capture` created).
    pub fn from_parts(device: ID3D11Device, context: ID3D11DeviceContext) -> Self {
        Self { device, context }
    }
}

/// An FFmpeg hardware-frame pool backed by a D3D11 NV12 array texture.
pub struct HwFrames {
    frames_ref: *mut ffi::AVBufferRef,
    texture: ID3D11Texture2D,
    width: u32,
    height: u32,
    pool: u32,
}

// The FFmpeg buffer ref is ref-counted and only touched from one thread at a time.
unsafe impl Send for HwFrames {}
unsafe impl Sync for HwFrames {}

impl HwFrames {
    /// Builds a D3D11VA hardware-device context around `device` and an NV12
    /// frame pool on top of it.
    ///
    /// Drivers differ in how many NV12 array slices they allow (some accept only
    /// one or two), so the largest working pool size at or below `pool` is used.
    pub fn new(device: &GpuDevice, width: u32, height: u32, pool: u32) -> Result<Self> {
        let (texture, slices) = create_nv12_texture(device, width, height, pool)?;
        Self::from_texture(device, texture, width, height, slices)
    }

    /// Builds the hardware frame pool on an already-created NV12 array texture.
    ///
    /// Letting the caller create and validate the texture (and the video
    /// processor that writes into it) first keeps the FFmpeg context from being
    /// built at all when the GPU path is unavailable.
    pub fn from_texture(
        device: &GpuDevice,
        texture: ID3D11Texture2D,
        width: u32,
        height: u32,
        slices: u32,
    ) -> Result<Self> {
        unsafe { Self::build(device, texture, width, height, slices) }
    }
}

/// Creates an NV12 array texture, halving the array size until the driver accepts.
///
/// Returns the texture and the array size it was created with.
pub fn create_nv12_texture(
    device: &GpuDevice,
    width: u32,
    height: u32,
    pool: u32,
) -> Result<(ID3D11Texture2D, u32)> {
    let mut array = pool.max(1);
    loop {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: array,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: ENCODER_TEXTURE_BIND_FLAGS,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };

        let mut texture: Option<ID3D11Texture2D> = None;
        match unsafe { device.device.CreateTexture2D(&desc, None, Some(&mut texture)) } {
            Ok(()) => {
                let texture =
                    texture.ok_or_else(|| anyhow!("D3D11 returned no NV12 texture"))?;
                return Ok((texture, array));
            }
            Err(error) => {
                if array <= 1 {
                    return Err(anyhow!("CreateTexture2D(NV12) failed: {error}"));
                }
                array /= 2;
            }
        }
    }
}

impl HwFrames {
    unsafe fn build(
        gpu: &GpuDevice,
        texture: ID3D11Texture2D,
        width: u32,
        height: u32,
        pool: u32,
    ) -> Result<Self> {
        // 1) D3D11VA device context around the shared device.
        let device_ref = ffi::av_hwdevice_ctx_alloc(ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA);
        if device_ref.is_null() {
            return Err(anyhow!("av_hwdevice_ctx_alloc returned null"));
        }

        {
            let device_ctx = (*device_ref).data as *mut ffi::AVHWDeviceContext;
            let d3d = (*device_ctx).hwctx as *mut ffi::AVD3D11VADeviceContext;

            // FFmpeg takes ownership of these references (releasing them in its
            // uninit callback), so hand over owned clones and forget them here.
            let owned_device = gpu.device.clone();
            let owned_context = gpu.context.clone();
            (*d3d).device = owned_device.as_raw() as *mut ffi::ID3D11Device;
            (*d3d).device_context = owned_context.as_raw() as *mut ffi::ID3D11DeviceContext;
            std::mem::forget(owned_device);
            std::mem::forget(owned_context);

            // Leave video_device/video_context null: FFmpeg queries them itself.
            (*d3d).video_device = ptr::null_mut();
            (*d3d).video_context = ptr::null_mut();
            (*d3d).BindFlags = ENCODER_TEXTURE_BIND_FLAGS;
            (*d3d).MiscFlags = 0;
        }

        if ffi::av_hwdevice_ctx_init(device_ref) < 0 {
            let mut device_ref = device_ref;
            ffi::av_buffer_unref(&mut device_ref);
            return Err(anyhow!("av_hwdevice_ctx_init(D3D11VA) failed"));
        }

        // 2) Hardware frame pool on that device, backed by our NV12 texture.
        let frames_ref = ffi::av_hwframe_ctx_alloc(device_ref);
        if frames_ref.is_null() {
            let mut device_ref = device_ref;
            ffi::av_buffer_unref(&mut device_ref);
            return Err(anyhow!("av_hwframe_ctx_alloc returned null"));
        }

        {
            let frames_ctx = (*frames_ref).data as *mut ffi::AVHWFramesContext;
            (*frames_ctx).format = ffi::AVPixelFormat::AV_PIX_FMT_D3D11;
            (*frames_ctx).sw_format = ffi::AVPixelFormat::AV_PIX_FMT_NV12;
            (*frames_ctx).width = width as i32;
            (*frames_ctx).height = height as i32;
            (*frames_ctx).initial_pool_size = pool as i32;

            let hw = (*frames_ctx).hwctx as *mut ffi::AVD3D11VAFramesContext;
            (*hw).texture = texture.as_raw() as *mut ffi::ID3D11Texture2D;
            (*hw).BindFlags = ENCODER_TEXTURE_BIND_FLAGS;
            (*hw).MiscFlags = 0;
        }

        if ffi::av_hwframe_ctx_init(frames_ref) < 0 {
            let mut frames_ref = frames_ref;
            let mut device_ref = device_ref;
            ffi::av_buffer_unref(&mut frames_ref);
            ffi::av_buffer_unref(&mut device_ref);
            return Err(anyhow!("av_hwframe_ctx_init(NV12) failed"));
        }

        // The frames context holds its own reference to the device context.
        let mut device_ref = device_ref;
        ffi::av_buffer_unref(&mut device_ref);

        Ok(Self {
            frames_ref,
            texture,
            width,
            height,
            pool,
        })
    }

    pub fn frames_ref(&self) -> *mut ffi::AVBufferRef {
        self.frames_ref
    }

    pub fn texture(&self) -> &ID3D11Texture2D {
        &self.texture
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pool(&self) -> u32 {
        self.pool
    }

    /// Borrows an NV12 frame from the pool.
    pub fn get_frame(&self) -> Result<HwFrame> {
        let frame = unsafe { ffi::av_frame_alloc() };
        if frame.is_null() {
            return Err(anyhow!("av_frame_alloc returned null"));
        }
        let rc = unsafe { ffi::av_hwframe_get_buffer(self.frames_ref, frame, 0) };
        if rc < 0 {
            let mut frame = frame;
            unsafe { ffi::av_frame_free(&mut frame) };
            return Err(hr(rc));
        }
        Ok(HwFrame {
            frame,
            width: self.width,
            height: self.height,
        })
    }
}

impl Drop for HwFrames {
    fn drop(&mut self) {
        unsafe {
            let mut frames_ref = self.frames_ref;
            ffi::av_buffer_unref(&mut frames_ref);
        }
    }
}

/// A borrowed NV12 hardware frame. Owns the underlying `AVFrame`.
pub struct HwFrame {
    frame: *mut ffi::AVFrame,
    width: u32,
    height: u32,
}

// `AVFrame` is only moved between the capture and encoder threads, never aliased.
unsafe impl Send for HwFrame {}

impl HwFrame {
    pub fn as_ptr(&self) -> *mut ffi::AVFrame {
        self.frame
    }

    /// Array slice FFmpeg picked inside the NV12 texture.
    pub fn slice_index(&self) -> usize {
        unsafe { (*self.frame).data[1] as usize }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn set_pts(&mut self, pts: i64) {
        unsafe {
            (*self.frame).pts = pts;
        }
    }

    /// Relinquishes ownership of the underlying `AVFrame` without freeing it.
    ///
    /// Used when handing the frame to `ffmpeg`, which then owns it.
    pub fn into_raw(mut self) -> *mut ffi::AVFrame {
        let frame = self.frame;
        self.frame = ptr::null_mut();
        std::mem::forget(self);
        frame
    }

    /// Copies this hardware NV12 frame down to a CPU NV12 `AVFrame`.
    ///
    /// Used on the Intel path, where no hardware encoder can take the texture.
    /// The returned frame is owned by the caller (free with `av_frame_free`).
    pub fn download_nv12(&self) -> Result<*mut ffi::AVFrame> {
        let dst = unsafe { ffi::av_frame_alloc() };
        if dst.is_null() {
            return Err(anyhow!("av_frame_alloc returned null"));
        }
        unsafe {
            (*dst).format = ffi::AVPixelFormat::AV_PIX_FMT_NV12 as i32;
            (*dst).width = self.width as i32;
            (*dst).height = self.height as i32;
        }
        let rc = unsafe { ffi::av_hwframe_transfer_data(dst, self.frame, 0) };
        if rc < 0 {
            let mut dst = dst;
            unsafe { ffi::av_frame_free(&mut dst) };
            return Err(hr(rc));
        }
        Ok(dst)
    }
}

impl Drop for HwFrame {
    fn drop(&mut self) {
        unsafe {
            let mut frame = self.frame;
            ffi::av_frame_free(&mut frame);
        }
    }
}

/// Converts a captured BGRA texture into one slice of the NV12 pool on the GPU.
pub struct VideoProcessor {
    device: ID3D11VideoDevice,
    context: ID3D11VideoContext,
    processor: ID3D11VideoProcessor,
    enumerator: ID3D11VideoProcessorEnumerator,
    output_views: Vec<ID3D11VideoProcessorOutputView>,
    width: u32,
    height: u32,
}

impl VideoProcessor {
    /// Creates a processor converting `width`x`height` BGRA into the NV12 array
    /// texture `texture` (which must have `slices` array elements).
    pub fn new(
        gpu: &GpuDevice,
        texture: &ID3D11Texture2D,
        slices: u32,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let device: ID3D11VideoDevice = gpu
            .device
            .cast()
            .map_err(|e| anyhow!("device has no ID3D11VideoDevice: {e}"))?;
        let context: ID3D11VideoContext = gpu
            .context
            .cast()
            .map_err(|e| anyhow!("device context has no ID3D11VideoContext: {e}"))?;

        let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            InputWidth: width,
            InputHeight: height,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            OutputWidth: width,
            OutputHeight: height,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };

        let (processor, enumerator) = unsafe {
            let enumerator = device
                .CreateVideoProcessorEnumerator(&desc)
                .map_err(|e| anyhow!("CreateVideoProcessorEnumerator failed: {e}"))?;
            let processor = device
                .CreateVideoProcessor(&enumerator, 0)
                .map_err(|e| anyhow!("CreateVideoProcessor failed: {e}"))?;
            (processor, enumerator)
        };

        // One output view per array slice, built once and reused.
        let mut output_views = Vec::with_capacity(slices as usize);
        for slice in 0..slices {
            let view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2DARRAY,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2DArray: D3D11_TEX2D_ARRAY_VPOV {
                        MipSlice: 0,
                        FirstArraySlice: slice,
                        ArraySize: 1,
                    },
                },
            };
            let mut view: Option<ID3D11VideoProcessorOutputView> = None;
            unsafe {
                device
                    .CreateVideoProcessorOutputView(texture, &enumerator, &view_desc, Some(&mut view))
                    .map_err(|e| anyhow!("CreateVideoProcessorOutputView failed: {e}"))?;
            }
            output_views.push(view.ok_or_else(|| anyhow!("no output view returned"))?);
        }

        Ok(Self {
            device,
            context,
            processor,
            enumerator,
            output_views,
            width,
            height,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Blits `source` (BGRA) into NV12 slice `slice`.
    pub unsafe fn convert(&self, source: &ID3D11Texture2D, slice: usize) -> Result<()> {
        let output = self
            .output_views
            .get(slice)
            .ok_or_else(|| anyhow!("slice {slice} outside pool"))?;

        let input_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };

        let mut input: Option<ID3D11VideoProcessorInputView> = None;
        self.device
            .CreateVideoProcessorInputView(
                source,
                &self.enumerator,
                &input_desc,
                Some(&mut input),
            )
            .map_err(|e| anyhow!("CreateVideoProcessorInputView failed: {e}"))?;
        let input = input.ok_or_else(|| anyhow!("no input view returned"))?;

        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: ptr::null_mut(),
            pInputSurface: ManuallyDrop::new(Some(input)),
            ppFutureSurfaces: ptr::null_mut(),
            ppPastSurfacesRight: ptr::null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: ptr::null_mut(),
        };

        self.context
            .VideoProcessorBlt(&self.processor, output, 0, std::slice::from_ref(&stream))
            .map_err(|e| anyhow!("VideoProcessorBlt failed: {e}"))?;
        Ok(())
    }
}
