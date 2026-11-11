//! OS-specific services behind one small API so the GUI compiles everywhere.

#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
pub use windows_impl::*;

#[cfg(not(windows))]
mod unix;
#[cfg(not(windows))]
pub use unix::*;
