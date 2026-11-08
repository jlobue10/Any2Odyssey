//! Windows.Graphics.Capture monitor capture. Only the newest frame is kept,
//! cursor capture is disabled (the live Windows pointer must never travel
//! through the delayed AI pipeline), and the yellow capture border is removed
//! when Windows grants the per-app Borderless permission.

use std::time::Duration;

use any2odyssey_core::status::CaptureBorderStatus;
use windows::core::Interface;
use windows::Foundation::TimeSpan;
use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureAccess,
    GraphicsCaptureAccessKind, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Security::Authorization::AppCapabilityAccess::AppCapabilityAccessStatus;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

pub struct WgcFrameSource {
    _device: IDirect3DDevice,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    item_size: SizeInt32,
    token: Option<i64>,
}

/// A checked-out pool frame. Return its buffer on every path, including errors.
pub struct CapturedFrame(Direct3D11CaptureFrame);

impl std::ops::Deref for CapturedFrame {
    type Target = Direct3D11CaptureFrame;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for CapturedFrame {
    fn drop(&mut self) {
        let _ = self.0.Close();
    }
}

// windows 0.62 represents a successful null interface as Error::empty(),
// whose code is S_OK. Failed HRESULTs must reach the worker's recovery path.
fn optional_frame<T>(result: windows::core::Result<T>) -> Result<Option<T>, String> {
    match result {
        Ok(frame) => Ok(Some(frame)),
        Err(e) if e.code().is_ok() => Ok(None),
        Err(e) => Err(format!("Capture frame acquisition: {e}")),
    }
}

/// Information about the border-permission request, for the status file.
pub struct BorderOutcome {
    pub access: String,
    pub border_required: bool,
    pub error: Option<String>,
}

impl WgcFrameSource {
    /// Start capturing `monitor` with `device`. `on_frame` is invoked on a
    /// capture thread for every arriving frame (the caller should just wake
    /// its own thread; use [`WgcFrameSource::take_latest`] to read).
    pub fn new(
        device: &ID3D11Device,
        monitor: HMONITOR,
        pixel_format: DirectXPixelFormat,
        on_frame: impl Fn() + Send + Sync + 'static,
        log: &dyn Fn(&str),
    ) -> windows::core::Result<(WgcFrameSource, BorderOutcome)> {
        let dxgi: IDXGIDevice = device.cast()?;
        let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? };
        let wrapped: IDirect3DDevice = inspectable.cast()?;
        let interop: IGraphicsCaptureItemInterop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
        let item_size = item.Size()?;
        let pool =
            Direct3D11CaptureFramePool::CreateFreeThreaded(&wrapped, pixel_format, 2, item_size)?;
        let session = pool.CreateCaptureSession(&item)?;
        session.SetIsCursorCaptureEnabled(false)?;
        if session.IsCursorCaptureEnabled()? {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL,
                "Windows did not disable cursor capture.",
            ));
        }
        let border = configure_border(&session, log);
        // 16 ms minimum interval; TimeSpan is in 100 ns units.
        session.SetMinUpdateInterval(TimeSpan { Duration: 160_000 })?;
        let handler = windows::Foundation::TypedEventHandler::<
            Direct3D11CaptureFramePool,
            windows::core::IInspectable,
        >::new(move |_, _| {
            on_frame();
            Ok(())
        });
        let token = pool.FrameArrived(&handler)?;
        session.StartCapture()?;
        log("WGC desktop capture started: cursor excluded, latest frame only, 16 ms minimum interval.");
        Ok((
            WgcFrameSource {
                _device: wrapped,
                pool,
                session,
                item_size,
                token: Some(token),
            },
            border,
        ))
    }

    /// Drain the pool and return the newest frame, or `None` when nothing new
    /// arrived. Errors if the monitor size changed (caller restarts).
    pub fn take_latest(&self) -> Result<Option<CapturedFrame>, String> {
        let Some(frame) = optional_frame(self.pool.TryGetNextFrame())? else {
            return Ok(None);
        };
        let mut latest = CapturedFrame(frame);
        for _ in 0..2 {
            match optional_frame(self.pool.TryGetNextFrame())? {
                Some(next) => latest = CapturedFrame(next),
                None => break,
            }
        }
        let size = latest.ContentSize().map_err(|e| e.to_string())?;
        if size.Width != self.item_size.Width || size.Height != self.item_size.Height {
            return Err("Monitor size changed. Restart conversion.".into());
        }
        Ok(Some(latest))
    }

    /// The D3D11 texture behind a frame's surface.
    pub fn texture(frame: &Direct3D11CaptureFrame) -> windows::core::Result<ID3D11Texture2D> {
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        unsafe { access.GetInterface::<ID3D11Texture2D>() }
    }
}

impl Drop for WgcFrameSource {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            let _ = self.pool.RemoveFrameArrived(token);
        }
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

/// Ask Windows for the per-app Borderless capture permission (never changes
/// global privacy settings) and drop the border when granted. The request runs
/// on a helper thread with a 10 s timeout so a stuck broker cannot hang capture.
fn configure_border(session: &GraphicsCaptureSession, log: &dyn Fn(&str)) -> BorderOutcome {
    let (tx, rx) = std::sync::mpsc::channel::<Result<AppCapabilityAccessStatus, String>>();
    std::thread::spawn(move || {
        let result = (|| -> Result<AppCapabilityAccessStatus, String> {
            let op =
                GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)
                    .map_err(|e| e.to_string())?;
            op.join().map_err(|e| e.to_string())
        })();
        let _ = tx.send(result);
    });
    let mut access = "Unavailable".to_owned();
    let mut error = None;
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(status)) => {
            access = format!("{status:?}");
            if status == AppCapabilityAccessStatus::Allowed {
                if let Err(e) = session.SetIsBorderRequired(false) {
                    error = Some(e.to_string());
                }
            }
        }
        Ok(Err(e)) => error = Some(e),
        Err(_) => error = Some("Borderless capture permission request timed out.".into()),
    }
    let border_required = session.IsBorderRequired().unwrap_or(true);
    log(&format!(
        "Capture border permission: {access}; border required: {border_required}{}",
        error.as_ref().map(|e| format!("; {e}")).unwrap_or_default()
    ));
    BorderOutcome {
        access,
        border_required,
        error,
    }
}

impl BorderOutcome {
    pub fn status(&self, pid: u32) -> CaptureBorderStatus {
        CaptureBorderStatus {
            pid,
            time: any2odyssey_core::log::now_iso(),
            access: self.access.clone(),
            border_required: self.border_required,
            error: self.error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{E_FAIL, RO_E_CLOSED};

    #[test]
    fn successful_null_frame_is_idle_but_failed_hresult_is_not() {
        assert_eq!(
            optional_frame::<u8>(Err(windows::core::Error::empty())),
            Ok(None)
        );
        assert_eq!(optional_frame(Ok(42)), Ok(Some(42)));
        for code in [E_FAIL, RO_E_CLOSED] {
            let error =
                optional_frame::<u8>(Err(windows::core::Error::from_hresult(code))).unwrap_err();
            assert!(error.contains("Capture frame acquisition"));
        }
    }
}
