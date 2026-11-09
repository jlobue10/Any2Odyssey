fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/Any2Odyssey.ico");
    println!("cargo:rerun-if-changed=app.manifest");
    // The About page shows the commit a build came from. Absent outside a
    // git checkout (source archives), which the page reports as such.
    if let Some(commit) = git_commit() {
        println!("cargo:rustc-env=ANY2ODYSSEY_GIT_SHA={commit}");
    }
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

fn git_commit() -> Option<String> {
    // HEAD is usually a symbolic ref whose contents stay unchanged across
    // commits. Watch its target too, and ask Git for paths so linked worktrees
    // (where .git is a file) use the right metadata directory.
    watch_git_path("HEAD");
    if let Some(reference) = git_output(&["symbolic-ref", "-q", "HEAD"]) {
        watch_git_path(&reference);
        watch_git_path("packed-refs");
    }
    let commit = git_output(&["rev-parse", "--short=12", "HEAD"])?;
    (!commit.is_empty() && commit.bytes().all(|b| b.is_ascii_hexdigit())).then_some(commit)
}

fn watch_git_path(name: &str) {
    let Some(path) = git_output(&["rev-parse", "--git-path", name]) else {
        return;
    };
    let path = std::path::Path::new(&path);
    // If packed-refs does not exist, the branch has a loose ref already
    // watched above. Its removal during packing will rerun this script.
    if name == "packed-refs" && !path.exists() {
        return;
    }
    // A branch stored only in packed-refs can gain a loose ref on its next
    // commit. Watch its nearest existing directory until that file appears;
    // watching a missing file would force every Cargo build to rerun.
    if let Some(existing) = path.ancestors().find(|candidate| candidate.exists()) {
        println!("cargo:rerun-if-changed={}", existing.display());
    }
}

fn git_output(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_owned())
}
