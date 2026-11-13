//! Windows.Graphics.Capture monitor capture. Only the newest frame is kept,
//! cursor capture is disabled (the live Windows pointer must never travel
//! through the delayed AI pipeline), and the yellow capture border is removed
//! when Windows grants the per-app Borderless permission.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
    /// Kept for its `Closed` event, which Windows raises when the captured
    /// display goes away; nothing else in the pipeline reports that.
    item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    item_size: SizeInt32,
    token: Option<i64>,
    closed: Arc<AtomicBool>,
    closed_token: Option<i64>,
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

/// The minimum update interval used when the display's refresh rate is
/// unknown: 16 ms, in the 100 ns units of a `TimeSpan`.
const DEFAULT_MIN_UPDATE_INTERVAL: i64 = 160_000;

/// A refresh rate the cadence can be derived from. Anything outside this
/// range is a failed or nonsensical query, not a display.
fn plausible_refresh(refresh_hz: Option<f64>) -> Option<f64> {
    refresh_hz.filter(|hz| (24.0..=1000.0).contains(hz))
}

/// How many display refreshes pass per delivered frame: as close to 60
/// frames a second as a whole number of refreshes gets.
fn refreshes_per_frame(refresh_hz: f64) -> f64 {
    (refresh_hz / 60.0).round().max(1.0)
}

/// The capture session's minimum update interval, in 100 ns units, for a
/// display refreshing at `refresh_hz`.
///
/// Windows delivers a frame at the first refresh after the interval has
/// passed, so an interval close to a multiple of the refresh period beats
/// against it: the fixed 16 ms left 0.67 ms of slack at 120 Hz (two refreshes
/// are 16.67 ms), and ordinary timing jitter slipped frames to the third
/// refresh (57 fps measured); at 144 Hz it selected every third refresh
/// (48 fps). Half a refresh short of the wanted number of refreshes puts the
/// deadline midway between two of them. A rate that is unknown or implausible
/// keeps the old 16 ms.
pub fn min_update_interval(refresh_hz: Option<f64>) -> i64 {
    let Some(hz) = plausible_refresh(refresh_hz) else {
        return DEFAULT_MIN_UPDATE_INTERVAL;
    };
    let period = 10_000_000.0 / hz;
    ((refreshes_per_frame(hz) - 0.5) * period).round() as i64
}

/// The cadence part of the capture-started log line.
fn cadence_note(refresh_hz: Option<f64>) -> String {
    let millis = min_update_interval(refresh_hz) as f64 / 10_000.0;
    match (plausible_refresh(refresh_hz), refresh_hz) {
        (Some(hz), _) => format!(
            "{millis:.2} ms minimum interval (display at {hz:.2} Hz, one frame per {} refresh(es))",
            refreshes_per_frame(hz)
        ),
        (None, Some(hz)) => format!(
            "{millis:.2} ms minimum interval (the reported refresh rate, {hz:.2} Hz, was not used)"
        ),
        (None, None) => format!("{millis:.2} ms minimum interval (display refresh rate unknown)"),
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
    /// `refresh_hz` is the monitor's refresh rate when known; it sets the
    /// frame cadence, see [`min_update_interval`].
    pub fn new(
        device: &ID3D11Device,
        monitor: HMONITOR,
        pixel_format: DirectXPixelFormat,
        refresh_hz: Option<f64>,
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
        let session = match pool.CreateCaptureSession(&item) {
            Ok(session) => session,
            Err(e) => {
                let _ = pool.Close();
                return Err(e);
            }
        };
        // Configure and start in one fallible step so every failure closes
        // the session and pool instead of leaving them to a plain Release.
        let closed = Arc::new(AtomicBool::new(false));
        let configured = (|| -> windows::core::Result<(BorderOutcome, i64, i64)> {
            session.SetIsCursorCaptureEnabled(false)?;
            if session.IsCursorCaptureEnabled()? {
                return Err(windows::core::Error::new(
                    windows::Win32::Foundation::E_FAIL,
                    "Windows did not disable cursor capture.",
                ));
            }
            let border = configure_border(&session, log);
            session.SetMinUpdateInterval(TimeSpan {
                Duration: min_update_interval(refresh_hz),
            })?;
            let handler = windows::Foundation::TypedEventHandler::<
                Direct3D11CaptureFramePool,
                windows::core::IInspectable,
            >::new(move |_, _| {
                on_frame();
                Ok(())
            });
            let token = pool.FrameArrived(&handler)?;
            // The owner polls `take_latest` from its update timer, so the
            // flag needs no wake of its own.
            let closed_handler = windows::Foundation::TypedEventHandler::<
                GraphicsCaptureItem,
                windows::core::IInspectable,
            >::new({
                let closed = closed.clone();
                move |_, _| {
                    closed.store(true, Ordering::Release);
                    Ok(())
                }
            });
            let closed_token = match item.Closed(&closed_handler) {
                Ok(closed_token) => closed_token,
                Err(e) => {
                    let _ = pool.RemoveFrameArrived(token);
                    return Err(e);
                }
            };
            if let Err(e) = session.StartCapture() {
                let _ = item.RemoveClosed(closed_token);
                let _ = pool.RemoveFrameArrived(token);
                return Err(e);
            }
            Ok((border, token, closed_token))
        })();
        let (border, token, closed_token) = match configured {
            Ok(v) => v,
            Err(e) => {
                let _ = session.Close();
                let _ = pool.Close();
                return Err(e);
            }
        };
        log(&format!(
            "WGC desktop capture started: cursor excluded, latest frame only, {}.",
            cadence_note(refresh_hz)
        ));
        Ok((
            WgcFrameSource {
                _device: wrapped,
                item,
                pool,
                session,
                item_size,
                token: Some(token),
                closed,
                closed_token: Some(closed_token),
            },
            border,
        ))
    }

    /// Drain the pool and return the newest frame, or `None` when nothing new
    /// arrived. Errors if the monitor size changed or the captured display
    /// went away (caller restarts).
    pub fn take_latest(&self) -> Result<Option<CapturedFrame>, String> {
        // A closed item delivers no more frames, and an empty pool is "idle",
        // not an error: without this the last image would stay up for good.
        if self.closed.load(Ordering::Acquire) {
            return Err(
                "Capture item closed: the display was removed or changed. Restart conversion."
                    .into(),
            );
        }
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
        // Handlers first, so neither callback runs against a session or pool
        // that is being closed; the item itself is released after both.
        if let Some(token) = self.token.take() {
            let _ = self.pool.RemoveFrameArrived(token);
        }
        if let Some(token) = self.closed_token.take() {
            let _ = self.item.RemoveClosed(token);
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
    fn the_capture_interval_sits_half_a_refresh_before_the_wanted_one() {
        // (refresh rate, refreshes per frame, resulting frames per second)
        for (hz, refreshes, fps) in [
            (60.0, 1.0, 60.0),
            (59.94, 1.0, 59.94),
            (75.0, 1.0, 75.0),
            (100.0, 2.0, 50.0),
            (120.0, 2.0, 60.0),
            (144.0, 2.0, 72.0),
            (165.0, 3.0, 55.0),
            (240.0, 4.0, 60.0),
        ] {
            let interval = min_update_interval(Some(hz)) as f64;
            let period = 10_000_000.0 / hz;
            assert_eq!(refreshes_per_frame(hz), refreshes, "{hz} Hz");
            // Half a period of slack on both sides of the deadline: the
            // refresh before it is too early, the wanted one is late enough.
            assert!(
                (interval - (refreshes - 0.5) * period).abs() <= 0.5,
                "{hz} Hz: {interval}"
            );
            assert!(interval > (refreshes - 1.0) * period && interval < refreshes * period);
            assert!((hz / refreshes - fps).abs() < 1e-9, "{hz} Hz");
        }
        assert_eq!(min_update_interval(Some(120.0)), 125_000);
        assert_eq!(min_update_interval(Some(60.0)), 83_333);
        assert_eq!(min_update_interval(Some(24.0)), 208_333);
        // Unknown or implausible: the value every earlier version used.
        for unknown in [
            None,
            Some(0.0),
            Some(23.9),
            Some(1000.1),
            Some(-60.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
        ] {
            assert_eq!(min_update_interval(unknown), 160_000, "{unknown:?}");
        }
    }

    #[test]
    fn the_start_line_names_the_cadence_and_where_it_came_from() {
        assert_eq!(
            cadence_note(Some(120.0)),
            "12.50 ms minimum interval (display at 120.00 Hz, one frame per 2 refresh(es))"
        );
        assert_eq!(
            cadence_note(None),
            "16.00 ms minimum interval (display refresh rate unknown)"
        );
        assert_eq!(
            cadence_note(Some(5.0)),
            "16.00 ms minimum interval (the reported refresh rate, 5.00 Hz, was not used)"
        );
    }

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
