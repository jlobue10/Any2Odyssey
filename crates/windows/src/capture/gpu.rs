//! GPU desktop capture for one monitor: WGC frames (HDR R16G16B16A16 or SDR
//! BGRA) are converted to SDR BGRA on the GPU and either presented into an
//! offscreen surface or read back into a CPU buffer for tests.
//!
//! Each acquired frame is a complete current desktop image. It is never
//! blended with the previous frame, so vanished menus disappear immediately.

use any2odyssey_core::capture_color::{CaptureColor, ColorMonitor};
use any2odyssey_core::geometry::Rect;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};
use windows::core::{s, Interface, PCSTR};
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11PixelShader,
    ID3D11RenderTargetView, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11_VIEWPORT,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_MODE_ROTATION_IDENTITY,
    DXGI_MODE_ROTATION_UNSPECIFIED, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIDevice1, IDXGIFactory1, IDXGIOutput,
};
use windows::Win32::Graphics::Gdi::HMONITOR;

use super::surface::GpuSurface;
use super::wgc::{BorderOutcome, WgcFrameSource};
use crate::display_color;
use crate::win::rect_from;

pub struct GpuDesktopCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    source: Option<WgcFrameSource>,
    pub border: Option<BorderOutcome>,
    staging: Option<ID3D11Texture2D>,
    hdr: Option<HdrConverter>,
    sdr_input: Option<ID3D11Texture2D>,
    surface: Option<GpuSurface>,
    color: ColorMonitor,
    color_checked: Instant,
    /// Asks Windows for the display's colour state off this thread; `None`
    /// when its thread could not be started, and the query runs here.
    color_watch: Option<ColorWatch>,
    device_name: String,
    capture: Rect,
    output_bounds: Rect,
    pub frames: u64,
}

struct HdrConverter {
    input: ID3D11Texture2D,
    view: ID3D11ShaderResourceView,
    output: ID3D11Texture2D,
    target: ID3D11RenderTargetView,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
}

/// How often the display's colour state is asked for. Twice per check on
/// the capture thread (one a second), so a check normally finds an answer
/// newer than the last one it saw.
const COLOR_QUERY_PERIOD: Duration = Duration::from_millis(500);

/// The display-colour query on a thread of its own.
///
/// The query enumerates the display configuration and asks for three device
/// properties. It used to run on the capture thread once a second, where
/// every millisecond it takes (many more while the display topology is
/// changing) is taken from a frame. This thread repeats it and keeps the
/// newest answer for the capture thread to collect. It uses no COM and
/// shares nothing but that slot; dropping the watch tells it to stop, and
/// it is not joined, so a query that never returns cannot hold up shutdown.
struct ColorWatch {
    shared: Arc<ColorWatchShared>,
}

struct ColorWatchShared {
    state: Mutex<ColorWatchState>,
    wake: Condvar,
}

#[derive(Default)]
struct ColorWatchState {
    /// The newest answer not yet collected; `Some(None)` is a failed query.
    latest: Option<Option<CaptureColor>>,
    stop: bool,
}

impl ColorWatch {
    fn start(
        mut query: impl FnMut() -> Option<CaptureColor> + Send + 'static,
        period: Duration,
    ) -> std::io::Result<ColorWatch> {
        let shared = Arc::new(ColorWatchShared {
            state: Mutex::new(ColorWatchState::default()),
            wake: Condvar::new(),
        });
        let thread_shared = shared.clone();
        std::thread::Builder::new()
            .name("display-color".into())
            .spawn(move || loop {
                let state = thread_shared
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                let (state, _) = thread_shared
                    .wake
                    .wait_timeout_while(state, period, |state| !state.stop)
                    .unwrap_or_else(PoisonError::into_inner);
                if state.stop {
                    return;
                }
                // Not under the lock: the capture thread must never wait
                // for a query.
                drop(state);
                let answer = query();
                thread_shared
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .latest = Some(answer);
            })?;
        Ok(ColorWatch { shared })
    }

    /// The newest answer since the last call, if a query finished since.
    fn take(&self) -> Option<Option<CaptureColor>> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .latest
            .take()
    }
}

impl Drop for ColorWatch {
    fn drop(&mut self) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop = true;
        self.shared.wake.notify_all();
    }
}

/// Monitor lookup result used before any device is created.
struct OutputMatch {
    adapter: windows::Win32::Graphics::Dxgi::IDXGIAdapter1,
    bounds: Rect,
    monitor: HMONITOR,
}

fn find_output(device_name: &str) -> Result<OutputMatch, String> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(|e| e.to_string())?;
    let mut a = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(a) } {
        a += 1;
        let mut o = 0;
        while let Ok(output) = unsafe { adapter.EnumOutputs(o) } {
            o += 1;
            let output: IDXGIOutput = output;
            let desc = unsafe { output.GetDesc() }.map_err(|e| e.to_string())?;
            let len = desc
                .DeviceName
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(desc.DeviceName.len());
            if String::from_utf16_lossy(&desc.DeviceName[..len]) != device_name {
                continue;
            }
            if desc.Rotation != DXGI_MODE_ROTATION_IDENTITY
                && desc.Rotation != DXGI_MODE_ROTATION_UNSPECIFIED
            {
                return Err(
                    "Desktop capture requires landscape orientation on the Odyssey.".into(),
                );
            }
            return Ok(OutputMatch {
                adapter,
                bounds: rect_from(desc.DesktopCoordinates),
                monitor: desc.Monitor,
            });
        }
    }
    Err("The Odyssey graphics output was not found.".into())
}

fn texture_desc(width: u32, height: u32) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: 0,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    }
}

fn create_texture(
    device: &ID3D11Device,
    desc: &D3D11_TEXTURE2D_DESC,
) -> windows::core::Result<ID3D11Texture2D> {
    let mut tex = None;
    unsafe { device.CreateTexture2D(desc, None, Some(&mut tex))? };
    Ok(tex.expect("texture created"))
}

impl GpuDesktopCapture {
    /// Capture `bounds` (desktop coordinates) of the monitor named
    /// `device_name` (e.g. `\\.\DISPLAY1`). `on_frame` wakes the owner.
    pub fn new(
        bounds: Rect,
        device_name: &str,
        on_frame: impl Fn() + Send + Sync + 'static,
        log: &dyn Fn(&str),
    ) -> Result<GpuDesktopCapture, String> {
        // A failed first query seeds the conservative fallback, and the first
        // real answer then costs a pipeline rebuild through recovery. The
        // query races display-topology changes, so give it a few tries first.
        let mut initial = display_color::capture_color(device_name);
        for _ in 0..4 {
            if initial.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            initial = display_color::capture_color(device_name);
        }
        if initial.is_none() {
            log("Display color query failed at startup; using FP16 until it answers.");
        }
        let color = ColorMonitor::new(initial);
        let found = find_output(device_name)?;
        if !found.bounds.contains_rect(&bounds) {
            return Err("Capture rectangle crosses a monitor boundary.".into());
        }
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                &found.adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map_err(|e| format!("Direct3D 11 device: {e}"))?;
        let device: ID3D11Device = device.ok_or("Direct3D 11 device was not created.")?;
        let context: ID3D11DeviceContext = context.ok_or("Direct3D 11 context was not created.")?;
        // ID3D11Multithread belongs to the immediate context, not the device.
        // WGC shares this device with our rendering path; do not silently run
        // without the protection we requested.
        let mt: ID3D11Multithread = context
            .cast()
            .map_err(|e| format!("Direct3D 11 multithread interface: {e}"))?;
        // The return value is the previous state, not a success indication.
        let _ = unsafe { mt.SetMultithreadProtected(true) };
        if !unsafe { mt.GetMultithreadProtected() }.as_bool() {
            return Err("Direct3D 11 multithread protection was not enabled.".into());
        }
        if let Ok(dxgi) = device.cast::<IDXGIDevice1>() {
            let _ = unsafe { dxgi.SetMaximumFrameLatency(1) };
        }
        let format = if color.color().advanced_color {
            DirectXPixelFormat::R16G16B16A16Float
        } else {
            DirectXPixelFormat::B8G8R8A8UIntNormalized
        };
        let (source, border) = WgcFrameSource::new(
            &device,
            found.monitor,
            format,
            display_color::refresh_rate(device_name),
            on_frame,
            log,
        )
        .map_err(|e| format!("Windows Graphics Capture: {e}"))?;
        log(&format!(
            "GPU desktop capture format: {format:?}; SDR white scale {}",
            color.color().white_scale
        ));
        let color_watch = {
            let name = device_name.to_owned();
            ColorWatch::start(
                move || display_color::capture_color(&name),
                COLOR_QUERY_PERIOD,
            )
        }
        .inspect_err(|e| {
            log(&format!(
                "Display color watch thread not started ({e}); querying on the capture thread."
            ))
        })
        .ok();
        Ok(GpuDesktopCapture {
            device,
            context,
            source: Some(source),
            border: Some(border),
            staging: None,
            hdr: None,
            sdr_input: None,
            surface: None,
            color,
            color_checked: Instant::now(),
            color_watch,
            device_name: device_name.to_owned(),
            capture: bounds,
            output_bounds: found.bounds,
            frames: 0,
        })
    }

    /// Present the newest frame into `window`'s swap chain. `Ok(false)` when
    /// no new frame was available.
    pub fn present(&mut self, window: HWND) -> Result<bool, String> {
        self.capture_into(Some(window), None)
    }

    /// Read the newest frame back into `pixels` (BGRA, tightly packed).
    pub fn read_back(&mut self, pixels: &mut Vec<u8>) -> Result<bool, String> {
        self.capture_into(None, Some(pixels))
    }

    fn capture_into(
        &mut self,
        window: Option<HWND>,
        pixels: Option<&mut Vec<u8>>,
    ) -> Result<bool, String> {
        // A format/white-level change invalidates this pipeline. The worker's
        // normal recovery path recreates it with the new display settings.
        if self.color_checked.elapsed() >= Duration::from_secs(1) {
            self.color_checked = Instant::now();
            // A removed or reset device delivers no more frames, and every
            // other call that could report it (the frame's texture, Present,
            // Map) runs only once a frame has arrived. Ask the device itself,
            // or the last image would stay on screen with nothing failing.
            // SAFETY: a plain query on the device this capture owns.
            unsafe { self.device.GetDeviceRemovedReason() }.map_err(|e| {
                format!("Direct3D device was removed or reset ({e}). Restart conversion.")
            })?;
            // No answer since the last check means the query has been
            // running for longer than that: counted like a failed query, so
            // a display state that cannot be read still stops the pipeline
            // after `MAX_QUERY_FAILURES` checks instead of being trusted.
            let observed = match &self.color_watch {
                Some(watch) => watch.take().unwrap_or(None),
                None => display_color::capture_color(&self.device_name),
            };
            self.color.observe(observed).map_err(str::to_owned)?;
        }
        let Some(source) = self.source.as_ref() else {
            return Err("Capture source closed.".into());
        };
        let Some(frame) = source.take_latest()? else {
            return Ok(false);
        };
        let texture = WgcFrameSource::texture(&frame).map_err(|e| e.to_string())?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        // The copies below assume a frame the size of the display as DXGI
        // reported it when the capture was built. The pool takes its size
        // from the capture item, read a moment later, so a mode change in
        // between leaves the two apart for good; Direct3D drops a copy
        // between different sizes without an error (black output), and a
        // source box outside the frame is undefined.
        let (width, height) = (
            self.output_bounds.width as u32,
            self.output_bounds.height as u32,
        );
        if desc.Width != width || desc.Height != height {
            return Err(format!(
                "Captured frames are {} x {} but the display is {width} x {height}. Restart conversion.",
                desc.Width, desc.Height
            ));
        }
        let full = self.capture == self.output_bounds;
        let current: ID3D11Texture2D = if desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT {
            self.ensure_hdr_converter()
                .map_err(|e| format!("HDR converter: {e}"))?;
            let hdr = self.hdr.as_ref().expect("ensured");
            self.copy_region(&texture, &hdr.input, full);
            unsafe {
                self.context.IASetInputLayout(None);
                self.context
                    .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                self.context.VSSetShader(&hdr.vs, None);
                self.context.PSSetShader(&hdr.ps, None);
                self.context
                    .PSSetShaderResources(0, Some(&[Some(hdr.view.clone())]));
                self.context
                    .OMSetRenderTargets(Some(&[Some(hdr.target.clone())]), None);
                self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                    TopLeftX: 0.0,
                    TopLeftY: 0.0,
                    Width: self.capture.width as f32,
                    Height: self.capture.height as f32,
                    MinDepth: 0.0,
                    MaxDepth: 1.0,
                }]));
                self.context.Draw(3, 0);
                self.context.OMSetRenderTargets(None, None);
                self.context.PSSetShaderResources(0, Some(&[None]));
            }
            hdr.output.clone()
        } else if desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM {
            if full {
                texture.clone()
            } else {
                if self.sdr_input.is_none() {
                    let d = texture_desc(self.capture.width as u32, self.capture.height as u32);
                    self.sdr_input =
                        Some(create_texture(&self.device, &d).map_err(|e| e.to_string())?);
                }
                let dst = self.sdr_input.clone().expect("created");
                self.copy_region(&texture, &dst, false);
                dst
            }
        } else {
            return Err(format!(
                "Unsupported desktop pixel format: {:?}",
                desc.Format
            ));
        };
        let result = if let Some(window) = window {
            if self.surface.is_none() {
                self.surface = Some(
                    GpuSurface::new(
                        &self.device,
                        window,
                        self.capture.width as u32,
                        self.capture.height as u32,
                    )
                    .map_err(|e| format!("Offscreen surface: {e}"))?,
                );
            }
            self.surface
                .as_ref()
                .expect("created")
                .present(&current)
                .map_err(|e| format!("Present: {e}"))
        } else if let Some(pixels) = pixels {
            self.read_pixels(&current, pixels)
        } else {
            Ok(())
        };
        result?;
        self.frames += 1;
        Ok(true)
    }

    fn read_pixels(
        &mut self,
        current: &ID3D11Texture2D,
        pixels: &mut Vec<u8>,
    ) -> Result<(), String> {
        // Only diagnostics read pixels back to the CPU. Production capture
        // should not reserve a 4K CPU-readable texture it never uses.
        if self.staging.is_none() {
            let mut desc = texture_desc(self.capture.width as u32, self.capture.height as u32);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            self.staging = Some(create_texture(&self.device, &desc).map_err(|e| e.to_string())?);
        }
        let staging = self.staging.as_ref().expect("created");
        unsafe { self.context.CopyResource(staging, current) };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(|e| e.to_string())?;
        let w = self.capture.width as usize;
        let h = self.capture.height as usize;
        pixels.clear();
        pixels.reserve(w * h * 4);
        for y in 0..h {
            let row = unsafe {
                std::slice::from_raw_parts(
                    (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                    w * 4,
                )
            };
            pixels.extend_from_slice(row);
        }
        unsafe { self.context.Unmap(staging, 0) };
        Ok(())
    }

    fn copy_region(&self, source: &ID3D11Texture2D, destination: &ID3D11Texture2D, full: bool) {
        if full {
            unsafe { self.context.CopyResource(destination, source) };
        } else {
            let b = D3D11_BOX {
                left: (self.capture.left() - self.output_bounds.left()) as u32,
                top: (self.capture.top() - self.output_bounds.top()) as u32,
                front: 0,
                right: (self.capture.right() - self.output_bounds.left()) as u32,
                bottom: (self.capture.bottom() - self.output_bounds.top()) as u32,
                back: 1,
            };
            unsafe {
                self.context
                    .CopySubresourceRegion(destination, 0, 0, 0, 0, source, 0, Some(&b))
            };
        }
    }

    fn ensure_hdr_converter(&mut self) -> windows::core::Result<()> {
        if self.hdr.is_some() {
            return Ok(());
        }
        let mut desc = texture_desc(self.capture.width as u32, self.capture.height as u32);
        desc.Format = DXGI_FORMAT_R16G16B16A16_FLOAT;
        desc.BindFlags = D3D11_BIND_SHADER_RESOURCE.0 as u32;
        let input = create_texture(&self.device, &desc)?;
        let mut view = None;
        unsafe {
            self.device
                .CreateShaderResourceView(&input, None, Some(&mut view))?
        };
        desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
        desc.BindFlags = D3D11_BIND_RENDER_TARGET.0 as u32;
        let output = create_texture(&self.device, &desc)?;
        let mut target = None;
        unsafe {
            self.device
                .CreateRenderTargetView(&output, None, Some(&mut target))?
        };
        let shader = hdr_to_sdr_shader(self.color.color().white_scale);
        let vs_code = compile(&shader, s!("VS"), s!("vs_5_0"))?;
        let ps_code = compile(&shader, s!("PS"), s!("ps_5_0"))?;
        let mut vs = None;
        let mut ps = None;
        unsafe {
            self.device
                .CreateVertexShader(&vs_code, None, Some(&mut vs))?;
            self.device
                .CreatePixelShader(&ps_code, None, Some(&mut ps))?;
        }
        self.hdr = Some(HdrConverter {
            input,
            view: view.expect("srv"),
            output,
            target: target.expect("rtv"),
            vs: vs.expect("vs"),
            ps: ps.expect("ps"),
        });
        Ok(())
    }
}

/// Full-screen triangle; scene-linear RGB divided by the SDR white scale,
/// saturated and encoded with the sRGB transfer function.
pub fn hdr_to_sdr_shader(white_scale: f32) -> String {
    format!(
        "Texture2D<float4> inputTexture:register(t0); \
         float4 VS(uint id:SV_VertexID):SV_POSITION {{float2 uv=float2((id<<1)&2,id&2);return float4(uv*float2(2,-2)+float2(-1,1),0,1);}} \
         float4 PS(float4 p:SV_POSITION):SV_TARGET {{float3 c=saturate(inputTexture.Load(int3(int2(p.xy),0)).rgb/{white_scale:.6});\
         float3 s=lerp(1.055*pow(c,1.0/2.4)-0.055,12.92*c,step(c,0.0031308));return float4(s,1);}}"
    )
}

fn compile(source: &str, entry: PCSTR, target: PCSTR) -> windows::core::Result<Vec<u8>> {
    let mut blob = None;
    let mut errors = None;
    let result = unsafe {
        D3DCompile(
            source.as_ptr() as *const _,
            source.len(),
            None,
            None,
            None,
            entry,
            target,
            0,
            0,
            &mut blob,
            Some(&mut errors),
        )
    };
    if let Err(e) = result {
        let message = errors
            .map(|b| {
                unsafe {
                    std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())
                }
                .to_vec()
            })
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .unwrap_or_default();
        return Err(windows::core::Error::new(
            e.code(),
            format!("shader compile failed: {message}"),
        ));
    }
    let blob = blob.expect("blob on success");
    Ok(unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
    }
    .to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    const HDR: CaptureColor = CaptureColor {
        advanced_color: true,
        white_scale: 2.5,
    };

    fn wait_for<T>(mut probe: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(found) = probe() {
                return found;
            }
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn the_color_watch_repeats_its_query_and_stops_when_dropped() {
        let queries = Arc::new(AtomicU32::new(0));
        let watch = {
            let queries = queries.clone();
            ColorWatch::start(
                move || {
                    queries.fetch_add(1, Ordering::SeqCst);
                    Some(HDR)
                },
                Duration::from_millis(5),
            )
            .unwrap()
        };
        assert_eq!(wait_for(|| watch.take()), Some(HDR));
        wait_for(|| (queries.load(Ordering::SeqCst) >= 3).then_some(()));
        drop(watch);
        // With the watch gone the count settles: the thread has ended.
        wait_for(|| {
            let before = queries.load(Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(40));
            (queries.load(Ordering::SeqCst) == before).then_some(())
        });
    }

    #[test]
    fn a_failed_query_is_an_answer_and_not_the_same_as_none_yet() {
        let watch = ColorWatch::start(|| None, Duration::from_millis(5)).unwrap();
        assert_eq!(wait_for(|| watch.take()), None);
    }

    #[test]
    fn an_answer_is_collected_once() {
        let watch = ColorWatch::start(|| Some(HDR), Duration::from_millis(5)).unwrap();
        wait_for(|| watch.take());
        // Stop the thread, let any query in flight publish, then drain: the
        // slot is empty from then on.
        watch
            .shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop = true;
        watch.shared.wake.notify_all();
        std::thread::sleep(Duration::from_millis(60));
        let _ = watch.take();
        assert_eq!(watch.take(), None);
        assert_eq!(watch.take(), None);
    }
}
