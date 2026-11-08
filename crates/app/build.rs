fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/Any2Odyssey.ico");
    // Version resource and icon for the Windows executable. Only when the
    // build host is Windows too; cross-checks from Linux skip it.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && cfg!(windows) {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/Any2Odyssey.ico");
        res.set("ProductName", "Any2Odyssey");
        res.set(
            "FileDescription",
            "Any2Odyssey - continuous desktop 3D for the Samsung Odyssey G90XF",
        );
        res.set(
            "LegalCopyright",
            "Copyright (C) 2026 Jonathan LoBue. GPL-3.0-only. Based on Any2Odd by Zhandragon.",
        );
        res.set("OriginalFilename", "Any2Odyssey.exe");
        if let Err(e) = res.compile() {
            println!("cargo:warning=Windows resource not embedded: {e}");
        }
    }
}
