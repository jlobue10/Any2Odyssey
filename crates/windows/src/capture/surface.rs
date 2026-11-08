//! A flip-model swap chain on a window; the capture pipeline copies each
//! converted frame into its back buffer and presents immediately.

use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11RenderTargetView, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISwapChain1, DXGI_MWA_NO_ALT_ENTER, DXGI_MWA_NO_PRINT_SCREEN,
    DXGI_MWA_NO_WINDOW_CHANGES, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

pub struct GpuSurface {
    device: ID3D11Device,
    swap_chain: IDXGISwapChain1,
    buffer: ID3D11Texture2D,
    target: ID3D11RenderTargetView,
}

impl GpuSurface {
    pub fn new(
        device: &ID3D11Device,
        window: HWND,
        width: u32,
        height: u32,
    ) -> windows::core::Result<GpuSurface> {
        let dxgi: IDXGIDevice = device.cast()?;
        let adapter = unsafe { dxgi.GetAdapter()? };
        let factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: width,
            Height: height,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            Flags: 0,
        };
        let swap_chain =
            unsafe { factory.CreateSwapChainForHwnd(device, window, &desc, None, None)? };
        unsafe {
            factory.MakeWindowAssociation(
                window,
                DXGI_MWA_NO_WINDOW_CHANGES | DXGI_MWA_NO_ALT_ENTER | DXGI_MWA_NO_PRINT_SCREEN,
            )?
        };
        let buffer: ID3D11Texture2D = unsafe { swap_chain.GetBuffer(0)? };
        let mut target = None;
        unsafe { device.CreateRenderTargetView(&buffer, None, Some(&mut target))? };
        Ok(GpuSurface {
            device: device.clone(),
            swap_chain,
            buffer,
            target: target.expect("created"),
        })
    }

    /// Copy `image` (same size and format as the back buffer) and present.
    pub fn present(&self, image: &ID3D11Texture2D) -> windows::core::Result<()> {
        let context = unsafe { self.device.GetImmediateContext()? };
        unsafe { context.CopyResource(&self.buffer, image) };
        unsafe { self.swap_chain.Present(0, DXGI_PRESENT(0)) }.ok()
    }

    /// Fill with a solid colour and present (diagnostics).
    pub fn clear(&self, rgb: [u8; 3]) -> windows::core::Result<()> {
        let context = unsafe { self.device.GetImmediateContext()? };
        let color = [
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
            1.0,
        ];
        unsafe { context.ClearRenderTargetView(&self.target, &color) };
        unsafe { self.swap_chain.Present(0, DXGI_PRESENT(0)) }.ok()
    }
}
