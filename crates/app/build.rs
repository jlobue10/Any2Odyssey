fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/Any2Odyssey.ico");
    println!("cargo:rerun-if-changed=app.manifest");
    // Version resource and icon for the Windows executable. Only when the
    // build host is Windows too; cross-checks from Linux skip it.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && cfg!(windows) {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/Any2Odyssey.ico");
        // Worker and diagnostics bypass the GUI event loop's DPI setup.
        // Set awareness before any mode performs monitor or window queries.
        res.set_manifest_file("app.manifest");
        let four_part = format!("{}.0", env!("CARGO_PKG_VERSION"));
        res.set("FileVersion", &four_part);
        res.set("ProductVersion", &four_part);
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
        res.compile()
            .expect("Windows resources, including the DPI manifest, must be embedded");
    }
}
