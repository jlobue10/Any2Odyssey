//! The numeric file version from an executable's version resource.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::{w, PCWSTR};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
};

/// `VS_FIXEDFILEINFO::dwSignature` of a valid block.
const FIXED_INFO_SIGNATURE: u32 = 0xFEEF_04BD;

/// `(major, minor, build, revision)` from the fixed part of `path`'s version
/// resource, the FILEVERSION a resource script sets. `None` when the file
/// cannot be read, carries no version resource or the block is malformed;
/// callers decide what an unknown version means.
pub fn file_version(path: &Path) -> Option<(u16, u16, u16, u16)> {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let name = PCWSTR(wide.as_ptr());
    // SAFETY: `name` points at `wide`, a NUL-terminated buffer that outlives
    // every call below.
    let size = unsafe { GetFileVersionInfoSizeW(name, None) };
    if size == 0 {
        return None;
    }
    // u32 elements keep the block aligned for the structure read out of it.
    let mut block = vec![0u32; (size as usize).div_ceil(4)];
    // SAFETY: `block` holds at least `size` writable bytes.
    unsafe { GetFileVersionInfoW(name, None, size, block.as_mut_ptr().cast()) }.ok()?;
    let mut info: *mut c_void = std::ptr::null_mut();
    let mut length = 0u32;
    // SAFETY: `block` was filled by GetFileVersionInfoW; the out-pointers are
    // valid for the call, and the returned pointer refers into `block`.
    let found = unsafe { VerQueryValueW(block.as_ptr().cast(), w!("\\"), &mut info, &mut length) };
    if !found.as_bool()
        || info.is_null()
        || (length as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
    {
        return None;
    }
    // SAFETY: the length check above covers the whole structure, and `block`
    // is still alive.
    let fixed = unsafe { info.cast::<VS_FIXEDFILEINFO>().read_unaligned() };
    if fixed.dwSignature != FIXED_INFO_SIGNATURE {
        return None;
    }
    Some(split(fixed.dwFileVersionMS, fixed.dwFileVersionLS))
}

/// The four 16-bit parts packed into the two version DWORDs.
fn split(most: u32, least: u32) -> (u16, u16, u16, u16) {
    (
        (most >> 16) as u16,
        most as u16,
        (least >> 16) as u16,
        least as u16,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_words_split_into_four_parts() {
        assert_eq!(split(0x0000_0008, 0x0006_0000), (0, 8, 6, 0));
        assert_eq!(split(0x000A_0000, 0x6658_1234), (10, 0, 0x6658, 0x1234));
        assert_eq!(
            split(u32::MAX, u32::MAX),
            (u16::MAX, u16::MAX, u16::MAX, u16::MAX)
        );
    }

    #[test]
    fn a_system_library_has_a_version_and_a_missing_file_has_none() {
        let system = std::env::var_os("SystemRoot").expect("SystemRoot");
        let kernel32 = Path::new(&system).join("System32").join("kernel32.dll");
        let (major, ..) = file_version(&kernel32).expect("kernel32.dll has a version resource");
        assert!(major >= 6, "{major}");
        assert_eq!(
            file_version(&kernel32.with_file_name("no-such-file.dll")),
            None
        );
        // A file that exists but carries no version resource.
        let dir = std::env::temp_dir().join(format!("any2odyssey-fv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let plain = dir.join("plain.txt");
        std::fs::write(&plain, b"not an executable").unwrap();
        assert_eq!(file_version(&plain), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
