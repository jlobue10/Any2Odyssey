//! Windows-only half of Any2Odyssey.
//!
//! Everything in this crate talks to Win32, DXGI/Direct3D 11, Windows Graphics
//! Capture or Samsung's `ConversionWrapper.dll`. It is compiled only for
//! Windows targets; on other platforms the crate is empty so the workspace
//! still builds.

#![cfg(windows)]
#![allow(clippy::missing_safety_doc)]

pub mod capture;
pub mod devices;
pub mod diagnostics;
pub mod display_color;
pub mod engine;
pub mod hotkeys;
pub mod ipc;
pub mod mirror;
pub mod overlay;
pub mod processes;
pub mod taskbar;
pub mod update;
pub mod win;
pub mod worker;

pub use windows;
