//! Desktop capture: Windows Graphics Capture frames on the GPU, converted to
//! SDR BGRA, presented into an offscreen swap chain or read back for tests.

pub mod gpu;
pub mod surface;
pub mod wgc;

pub use gpu::GpuDesktopCapture;
pub use surface::GpuSurface;
pub use wgc::WgcFrameSource;
