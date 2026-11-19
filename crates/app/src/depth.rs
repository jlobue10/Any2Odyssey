//! Depth from a single image, for the stereo viewer's 2D → 3D mode.
//!
//! The model is Depth Anything V2 (small) in ONNX form, run by the system's
//! ONNX Runtime. Neither ships inside the program: the runtime library is
//! opened and the model file read when the first depth map is asked for, and
//! a missing one is an ordinary error message with what to install.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use any2odyssey_core::dibr::DepthMap;
use any2odyssey_core::paths;
use image::RgbaImage;
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;

/// File name of the model in a models folder.
pub const MODEL_FILE: &str = "depth-anything-v2-small.onnx";
/// Environment override for the model file.
pub const MODEL_ENV: &str = "ANY2ODYSSEY_DEPTH_MODEL";
/// Environment override for the runtime library, as the binding itself reads.
const RUNTIME_ENV: &str = "ORT_DYLIB_PATH";
const RUNTIME_LIBRARY: &str = "libonnxruntime.so";

/// Where `--install-depth-model` gets the model: the ONNX export of Depth
/// Anything V2 Small (Apache-2.0) published by the ONNX community on Hugging
/// Face, and the SHA-256 of that file. A different file is refused. The
/// address names the commit of that repository that holds the file with
/// this hash, not its newest state, so that a later change there cannot
/// turn every installation into a failed check.
pub const MODEL_URL: &str =
    "https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/4472b7362082ad9968fee890ca0f1e5aca36b93d/onnx/model.onnx";
pub const MODEL_SHA256: &str = "afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c";
/// The most `curl` may fetch: the model is 99 MB, and this is well above it.
const MODEL_MAX_BYTES: u64 = 200 * 1024 * 1024;
/// Downloads that were given up (a killed installer) are cleared away by a
/// later one when nobody holds their lock any more, and where the folder
/// has no locks once they are this old: far longer than a download takes,
/// so one that is still running in another terminal is never touched.
const STALE_DOWNLOAD: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// The model works on patches of 14 pixels; its training size is 518.
const PATCH: u32 = 14;
/// The long side of the picture the model sees, in pixels: the size it was
/// trained at, and larger ones for finer depth (thin things and edges are
/// found better; each step costs about twice the time of the one before).
pub const DETAIL: [(u32, &str); 4] = [
    (518, "Standard"),
    (770, "Fine"),
    (1022, "Finer"),
    (1274, "Finest (slow without a GPU)"),
];
/// The size the model was trained at.
pub const LONG_SIDE: u32 = DETAIL[0].0;
/// ImageNet statistics the model's inputs are normalized with.
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// Which of the places a model is looked for a file was found in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// `$ANY2ODYSSEY_DEPTH_MODEL`: a model of the user's own choosing.
    Override,
    /// The user's data folder, where `--install-depth-model` puts it.
    Data,
    /// A system-wide folder a package installs into.
    System,
}

/// Where a model file is looked for, in order: the override, the user's data
/// folder, then the system-wide folders a package installs into.
fn model_candidates() -> Vec<(PathBuf, Origin)> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os(MODEL_ENV).filter(|p| !p.is_empty()) {
        candidates.push((PathBuf::from(path), Origin::Override));
    }
    candidates.push((
        paths::data_dir().join("models").join(MODEL_FILE),
        Origin::Data,
    ));
    for system in ["/usr/local/share/any2odyssey", "/usr/share/any2odyssey"] {
        candidates.push((
            Path::new(system).join("models").join(MODEL_FILE),
            Origin::System,
        ));
    }
    candidates
}

fn locate_model() -> Result<(PathBuf, Origin), String> {
    model_candidates()
        .into_iter()
        .find(|(path, _)| path.is_file())
        .ok_or_else(|| {
            format!(
                "The depth model is not installed. Put {MODEL_FILE} in {} (see Help: Windows & Linux).",
                paths::data_dir().join("models").display()
            )
        })
}

/// Check the model about to be loaded against [`MODEL_SHA256`], which until
/// now was only done when it was downloaded: a file that has been cut
/// short, damaged or exchanged since is refused before the runtime reads it.
///
/// Only the copy in the data folder is held to the hash. The override is
/// there to try a model of one's own, which is by design not the pinned
/// file. The system folders hold what the system's packager put there, as
/// trusted as this program itself and free to be another build of the model.
///
/// Reading 99 MB takes a moment, so the answer is remembered for as long as
/// the file keeps its size and time (`VERIFIED`): one reading per run.
/// Blocks: the caller is the thread that loads the model, never the UI's.
fn verify_installed(model: &Path, origin: Origin, expected: &str) -> Result<(), String> {
    static VERIFIED: Mutex<Option<(Identity, String)>> = Mutex::new(None);
    verify_installed_with(model, origin, expected, &VERIFIED)
}

/// What a file is recognized by between two looks at it.
type Identity = (PathBuf, u64, Option<std::time::SystemTime>);

fn verify_installed_with(
    model: &Path,
    origin: Origin,
    expected: &str,
    verified: &Mutex<Option<(Identity, String)>>,
) -> Result<(), String> {
    if origin != Origin::Data {
        return Ok(());
    }
    let unreadable =
        |e: std::io::Error| format!("Could not read the depth model {}: {e}", model.display());
    let metadata = std::fs::metadata(model).map_err(unreadable)?;
    let identity = (model.to_owned(), metadata.len(), metadata.modified().ok());
    let mut verified = verified.lock().unwrap_or_else(|e| e.into_inner());
    let actual = match &*verified {
        Some((known, actual)) if *known == identity => actual.clone(),
        _ => {
            let actual = any2odyssey_core::engine::sha256_hex(model).map_err(unreadable)?;
            *verified = Some((identity, actual.clone()));
            actual
        }
    };
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "The depth model {} is not the expected file (SHA-256 {actual}, expected {expected}): it is damaged or was replaced. Install it again with --install-depth-model. To use a model of your own, name it in ${MODEL_ENV}.",
            model.display()
        ))
    }
}

/// Download the model into the user's data folder with `curl`, check it
/// against [`MODEL_SHA256`] and only then give it its name. Returns where
/// it was put.
pub fn install_model() -> Result<PathBuf, String> {
    install_model_in(
        &paths::data_dir().join("models"),
        std::process::Command::new("curl"),
        MODEL_SHA256,
    )
}

fn install_model_in(
    folder: &Path,
    mut command: std::process::Command,
    expected: &str,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("Could not create {}: {e}", folder.display()))?;
    let target = folder.join(MODEL_FILE);
    remove_stale_downloads(folder, STALE_DOWNLOAD);
    // Exclusive creation keeps simultaneous installers independent and never
    // follows a pre-existing .part symlink. Curl writes through the already
    // open file, not a pathname it would reopen and truncate.
    //
    // The file keeps the permissions it is created with, the owner's alone
    // (0600), when it becomes the model: that is how everything else in the
    // data folder is kept (`fsutil::write_atomic`), and nobody but this
    // user's own program reads the model from there.
    let (partial, output) = start_download(folder)?;
    let status = command
        .args(download_arguments())
        .arg(MODEL_URL)
        .stdout(std::process::Stdio::from(output))
        .status()
        .map_err(|e| format!("Could not run curl ({e}). Install curl, or download {MODEL_URL} and save it as {}.", target.display()))?;
    if !status.success() {
        return Err(format!("The download failed (curl: {status})."));
    }
    verify_download(partial.path(), expected)?;
    partial
        .persist(&target)
        .map_err(|e| format!("Could not move the model into place: {e}"))?;
    Ok(target)
}

/// How a download in progress is named in the models folder.
const DOWNLOAD_PREFIX: &str = ".depth-download-";

/// Make the file a download goes to and the handle `curl` writes through.
///
/// That handle holds the file's lock for as long as it is open anywhere:
/// in this process until the installation is over, and in `curl`, which
/// inherits it. A later installer tells by it a download that was given up
/// (interrupted with Ctrl+C, a closed terminal, a kill: nobody holds the
/// lock) from one still running elsewhere ([`remove_stale_downloads`]).
/// Going by age alone left every interrupted download, up to 99 MB each,
/// for a day, and for ever when the next attempt worked and no installer
/// ran again.
fn start_download(folder: &Path) -> Result<(tempfile::NamedTempFile, std::fs::File), String> {
    let mut failed = String::from("another installer removed it");
    // Tried again when an installer that is clearing up took the new file
    // for an abandoned one in the moment before its lock was taken: once
    // for each installer starting at that moment, at the very most.
    for _ in 0..16 {
        let partial = tempfile::Builder::new()
            .prefix(DOWNLOAD_PREFIX)
            .tempfile_in(folder)
            .map_err(|e| format!("Could not create a model download: {e}"))?;
        match partial.reopen() {
            Ok(output) if claim(&output, partial.path()) => return Ok((partial, output)),
            Ok(_) => {}
            Err(e) => failed = e.to_string(),
        }
    }
    Err(format!("Could not open the model download: {failed}"))
}

/// Take the lock of the new download at `path`, of which `output` is an
/// open handle. False when an installer that is clearing up holds it (it
/// removes the file next) or has removed the file already.
fn claim(output: &std::fs::File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match crate::platform::try_lock_file(output) {
        Ok(true) => {}
        Ok(false) => return false,
        // A folder without locks (some network file systems): nobody can
        // take this file for an abandoned one before it is a day old.
        Err(_) => return true,
    }
    match (output.metadata(), std::fs::symlink_metadata(path)) {
        (Ok(open), Ok(named)) => open.dev() == named.dev() && open.ino() == named.ino(),
        _ => false,
    }
}

/// What `curl` is told, before the address.
fn download_arguments() -> Vec<String> {
    [
        // First, or curl has read it already: the user's ~/.curlrc is not
        // consulted, so nothing there (a proxy, `--insecure`, another
        // output) changes what this download does.
        "-q",
        "--fail",
        "--location",
        // HTTPS for the address and for every redirect it leads to.
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        // Not more than a model can be, where the server says the size
        // beforehand (newer curl also stops a longer transfer midway).
        "--max-filesize",
        &MODEL_MAX_BYTES.to_string(),
        // A server that does not answer, or a transfer that has stalled
        // (under 1 kB a second for a minute), ends instead of hanging.
        "--connect-timeout",
        "20",
        "--speed-limit",
        "1024",
        "--speed-time",
        "60",
        "--output",
        "-",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Clear away downloads that were given up: files in `folder` with the name
/// of a download in progress (and the `.part` file older versions used)
/// whose lock nobody holds ([`start_download`]) or that have not been
/// written to for `age`. Nothing else is touched, and nothing that is not
/// a plain file, a link least of all.
fn remove_stale_downloads(folder: &Path, age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let legacy = format!("{MODEL_FILE}.part");
    for entry in entries.flatten() {
        let name = entry.file_name();
        let ours = name
            .to_str()
            .is_some_and(|name| name.starts_with(DOWNLOAD_PREFIX) || name == legacy);
        // `DirEntry::metadata` does not follow a link.
        let Some(metadata) = entry.metadata().ok().filter(|m| ours && m.is_file()) else {
            continue;
        };
        let path = entry.path();
        // Held until the file is gone, so that an installer which made it
        // this moment and has not locked it yet finds out ([`claim`]).
        // Opened for writing as well: some network file systems give this
        // lock to nobody else.
        let unclaimed = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&path)
            .ok()
            .filter(|file| matches!(crate::platform::try_lock_file(file), Ok(true)));
        let old = metadata
            .modified()
            .ok()
            .and_then(|written| written.elapsed().ok())
            .is_some_and(|unwritten| unwritten >= age);
        if unclaimed.is_some() || old {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn verify_download(file: &Path, expected: &str) -> Result<(), String> {
    let actual = any2odyssey_core::engine::sha256_hex(file)
        .map_err(|e| format!("Could not read the downloaded file: {e}"))?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "The downloaded file is not the expected model (SHA-256 {actual}, expected {expected}). Nothing was installed."
        ))
    }
}

/// The size the image is given to the model at: the long side near
/// `long_side`, the aspect ratio kept, both sides a whole number of patches.
pub fn model_size(width: u32, height: u32, long_side: u32) -> (u32, u32) {
    let long = width.max(height).max(1) as f32;
    let long_side = long_side.clamp(DETAIL[0].0, DETAIL[DETAIL.len() - 1].0);
    let scale = long_side as f32 / long;
    let side = |pixels: u32| {
        let patches = (pixels as f32 * scale / PATCH as f32).round().max(1.0) as u32;
        patches * PATCH
    };
    (side(width), side(height))
}

/// `image` as the model's input: planar RGB, normalized, at `width` x `height`.
fn planar_input(image: &RgbaImage, width: u32, height: u32) -> Vec<f32> {
    // A live screen's frames come at the model's size already.
    let resized = if image.dimensions() == (width, height) {
        std::borrow::Cow::Borrowed(image)
    } else {
        std::borrow::Cow::Owned(image::imageops::resize(
            image,
            width,
            height,
            image::imageops::FilterType::Triangle,
        ))
    };
    let plane = width as usize * height as usize;
    let mut data = vec![0.0f32; plane * 3];
    for (index, pixel) in resized.pixels().enumerate() {
        for channel in 0..3 {
            data[channel * plane + index] =
                (pixel.0[channel] as f32 / 255.0 - MEAN[channel]) / STD[channel];
        }
    }
    data
}

/// Make cuDNN's functions available to ONNX Runtime's CUDA provider.
///
/// The provider library uses cuDNN, but some distributions' packages of it
/// (Arch's onnxruntime-cuda 1.29 among them) do not name cuDNN as a library
/// they need. The provider then fails to load over a cuDNN function it
/// cannot find, and depth silently runs on the processor, some fifteen
/// times slower. Opening cuDNN first, with its names visible to libraries
/// loaded later, is all the provider lacks. Without cuDNN installed nothing
/// is opened and nothing changes.
fn preload_cudnn() {
    use libloading::os::unix::{Library, RTLD_GLOBAL, RTLD_NOW};
    for name in ["libcudnn.so.9", "libcudnn.so.8", "libcudnn.so"] {
        // SAFETY: cuDNN is a library made to be loaded by programs like
        // this one; opening it runs only its own initializers.
        if let Ok(library) = unsafe { Library::open(Some(name), RTLD_NOW | RTLD_GLOBAL) } {
            // Kept for the life of the process: the provider uses it.
            std::mem::forget(library);
            return;
        }
    }
}

/// How many bytes the library file `path` has, and how many its own headers
/// say it has, when the second is the larger: a file that was cut short (a
/// copy or a download that was interrupted). `None` for a whole file, and
/// for anything that is not a library for this kind of processor, which the
/// system's loader refuses with a message of its own.
///
/// The loader maps a library by what its headers say and never compares
/// that with the file's length. Of a file that is cut short it maps pages
/// that are not there, and the first touch of one ends the whole program
/// with "Bus error", inside the loader, where nothing can be caught. So the
/// headers are read here first: the program headers and every segment they
/// load must lie inside the file, and so must the table of sections, which
/// is the last thing in a library as the linker writes it.
fn cut_short(path: &Path) -> Option<(u64, u64)> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut ident = [0u8; 16];
    file.read_exact(&mut ident).ok()?;
    let wide = cfg!(target_pointer_width = "64");
    let little = cfg!(target_endian = "little");
    if ident[..4] != *b"\x7fELF"
        || ident[4] != if wide { 2 } else { 1 }
        || ident[5] != if little { 1 } else { 2 }
    {
        return None;
    }
    let number = |bytes: &[u8]| -> u64 {
        let mut value = [0u8; 8];
        if little {
            value[..bytes.len()].copy_from_slice(bytes);
            u64::from_le_bytes(value)
        } else {
            value[8 - bytes.len()..].copy_from_slice(bytes);
            u64::from_be_bytes(value)
        }
    };
    // Where the fields lie, for 64 and for 32 bits: in the file's header the
    // two tables' places, entry sizes and counts; in a program header its
    // kind, its place in the file and its length there.
    let (header_size, tables, sizes, entry, offset, filesz) = if wide {
        (
            64usize,
            [0x20..0x28, 0x28..0x30],
            0x36usize,
            56u64,
            8..16,
            32..40,
        )
    } else {
        (52, [0x1C..0x20, 0x20..0x24], 0x2A, 32, 4..8, 16..20)
    };
    let mut header = vec![0u8; header_size];
    header[..16].copy_from_slice(&ident);
    if file.read_exact(&mut header[16..]).is_err() {
        // Not even its header is whole.
        return Some((length, header_size as u64));
    }
    let [program_table, section_table] = tables.map(|at| number(&header[at]));
    let field = |index: usize| number(&header[sizes + 2 * index..sizes + 2 * index + 2]);
    let (program_entry, programs, section_entry, sections) =
        (field(0), field(1), field(2), field(3));
    let mut needed = header_size as u64;
    // Zero where a library has no such table (a stripped one has no table
    // of sections), and all ones for a count that is kept elsewhere.
    if program_table != 0 && programs != 0xFFFF {
        needed = needed.max(program_table.saturating_add(program_entry * programs));
    }
    if section_table != 0 {
        needed = needed.max(section_table.saturating_add(section_entry * sections));
    }
    if needed <= length && program_table != 0 && programs != 0xFFFF && program_entry >= entry {
        let mut segment = vec![0u8; program_entry as usize];
        for index in 0..programs {
            let at = program_table + index * program_entry;
            if file.seek(SeekFrom::Start(at)).is_err() || file.read_exact(&mut segment).is_err() {
                return Some((length, at.saturating_add(program_entry)));
            }
            const LOAD: u64 = 1;
            if number(&segment[..4]) == LOAD {
                let end = number(&segment[offset.clone()])
                    .saturating_add(number(&segment[filesz.clone()]));
                needed = needed.max(end);
            }
        }
    }
    (needed > length).then_some((length, needed))
}

/// The folders the system's loader looks in for a library that is named
/// without a folder, as far as they can be known without asking it:
/// `LD_LIBRARY_PATH`, and where the distributions keep their libraries.
fn loader_folders() -> Vec<PathBuf> {
    let named = std::env::var_os("LD_LIBRARY_PATH").unwrap_or_default();
    let multiarch = format!("{}-linux-gnu", std::env::consts::ARCH);
    std::env::split_paths(&named)
        .filter(|folder| folder.is_absolute())
        .chain(
            [
                "/lib64",
                "/usr/lib64",
                "/lib",
                "/usr/lib",
                &format!("/lib/{multiarch}"),
                &format!("/usr/lib/{multiarch}"),
                "/usr/local/lib",
            ]
            .map(PathBuf::from),
        )
        .collect()
}

/// What to say instead of loading `library`, when a file the loader could
/// take for it is cut short ([`cut_short`]): the file itself where it is
/// named with its folder, otherwise one of that name in `folders`.
fn unloadable(library: &Path, folders: &[PathBuf]) -> Option<String> {
    let with_folder = library.components().count() > 1;
    let candidates: Vec<PathBuf> = if with_folder {
        vec![library.to_owned()]
    } else {
        folders.iter().map(|folder| folder.join(library)).collect()
    };
    candidates.iter().find_map(|file| {
        let (length, needed) = cut_short(file)?;
        Some(format!(
            "ONNX Runtime could not be loaded: {} is cut short (it has {length} bytes and its own headers describe {needed}), as an interrupted copy or download leaves it. Install or copy it again{}.",
            file.display(),
            if with_folder {
                ""
            } else {
                ", or name a whole library with ORT_DYLIB_PATH"
            }
        ))
    })
}

/// Load the runtime library once per process. The outcome is remembered: a
/// library that could not be loaded will not appear while the program runs.
fn runtime() -> Result<&'static Environment, String> {
    static LOADED: OnceLock<Result<Environment, String>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            let library = std::env::var_os(RUNTIME_ENV)
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(RUNTIME_LIBRARY));
            if let Some(refusal) = unloadable(&library, &loader_folders()) {
                return Err(refusal);
            }
            preload_cudnn();
            let builder = ort::init_from(&library).map_err(|e| {
                format!(
                    "ONNX Runtime could not be loaded ({}): {e}. It has to be version 1.{} or newer. Install your distribution's package (Arch: onnxruntime-cpu or onnxruntime-cuda; Debian and Ubuntu: libonnxruntime-dev; Fedora and openSUSE: onnxruntime-devel), or name a newer library with ORT_DYLIB_PATH.",
                    library.display(),
                    // (the binding's own number: the oldest runtime it was built to accept)
                    ort::MINOR_VERSION
                )
            })?;
            // The GPU is asked for where the model is loaded
            // (`Estimator::load`), which hears what the runtime answers.
            builder
                .with_name("any2odyssey")
                .build()
                .map_err(|e| format!("ONNX Runtime could not be started: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// The cores the model may use on the processor: a quarter of them, at
/// least two and at most six.
fn model_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    (cores / 4).clamp(2, 6)
}

/// Where the model runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunsOn {
    /// On an NVIDIA GPU, through the runtime's CUDA provider.
    Gpu,
    /// On the processor. `gpu_refused` is what the runtime said when the
    /// GPU could not be used although the installed runtime is one with
    /// the CUDA provider: it was installed for the GPU, and the model is
    /// several times slower than its user expects.
    Processor { gpu_refused: Option<String> },
}

impl RunsOn {
    /// One line for the page, under *Depth detail*.
    pub fn describe(&self) -> String {
        match self {
            RunsOn::Gpu => "Depth is computed on the NVIDIA GPU.".into(),
            RunsOn::Processor { gpu_refused: None } => "Depth is computed on the processor.".into(),
            RunsOn::Processor {
                gpu_refused: Some(why),
            } => format!(
                "Depth is computed on the processor, which is several times slower: the NVIDIA GPU could not be used ({}). View logs has the whole message; the GPU is tried again when the program is started again.",
                gist(why)
            ),
        }
    }

    /// The same for the log, with all of what the runtime said.
    pub fn for_log(&self) -> String {
        match self {
            RunsOn::Gpu => "Depth model: runs on the NVIDIA GPU (ONNX Runtime's CUDA provider).".into(),
            RunsOn::Processor { gpu_refused: None } => {
                "Depth model: runs on the processor (the installed ONNX Runtime has no CUDA provider).".into()
            }
            RunsOn::Processor {
                gpu_refused: Some(why),
            } => format!(
                "Depth model: runs on the processor, because the NVIDIA GPU could not be used. ONNX Runtime's CUDA provider said: {why}"
            ),
        }
    }
}

/// Where the model was last seen to run; nothing until it has been loaded.
static RUNS_ON: Mutex<Option<RunsOn>> = Mutex::new(None);

/// Where the model runs, once it has been loaded.
pub fn runs_on() -> Option<RunsOn> {
    RUNS_ON.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn note_runs_on(runs_on: &RunsOn) {
    *RUNS_ON.lock().unwrap_or_else(|e| e.into_inner()) = Some(runs_on.clone());
}

/// One of the runtime's messages without the list of addresses it appends
/// ("Stacktrace:" and a line for each frame): forty lines that say nothing
/// to the reader, in a status line.
fn without_trace(message: &str) -> &str {
    message
        .split("\nStacktrace:")
        .next()
        .unwrap_or(message)
        .trim_end()
}

/// The part of one of the runtime's messages that says what went wrong, at
/// a length the page can show. The runtime puts the names of its source
/// files and functions first and the cause last: CUDA's own complaint
/// where there is one, a library that could not be loaded, memory that
/// was not to be had, otherwise the end of the message.
fn gist(message: &str) -> String {
    const LENGTH: usize = 160;
    let message = without_trace(message);
    let from = |start: &str| message.rfind(start).map(|at| &message[at..]);
    let cause = if let Some(cuda) = from("CUDA failure") {
        // "CUDA failure 100: no CUDA-capable device is detected ; GPU=-1 ; ..."
        cuda.split(" ; ").next().unwrap_or(cuda)
    } else {
        from("Failed to load library")
            .or_else(|| from("Available memory of"))
            .or_else(|| from("Failed to allocate memory"))
            .unwrap_or(message)
    };
    let cause = cause.lines().last().unwrap_or(cause).trim();
    if cause.chars().count() <= LENGTH {
        return cause.to_owned();
    }
    let skip = cause.chars().count() - LENGTH;
    let tail: String = cause.chars().skip(skip).collect();
    // From a whole word on.
    let tail = tail.split_once(' ').map_or(tail.as_str(), |(_, rest)| rest);
    format!("…{tail}")
}

/// What a depth map is asked for, which decides what becomes of the room
/// the runtime took for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// A still image: no other map is wanted until the user opens another
    /// image or changes the depth detail.
    Still,
    /// A frame of a live screen: the next one follows at once, at the same
    /// size.
    Live,
}

/// Start the model on the GPU where the installed runtime has the CUDA
/// provider, and on the processor where it has none or the GPU cannot be
/// used; `open(true)` asks for the GPU. Fails with what the processor's
/// attempt said: that is the one that has to work.
///
/// The provider used to be asked for where the runtime is started, with
/// its failure to load thrown away. Two things came of that. A runtime
/// installed for the GPU that could not load the provider (cuDNN or CUDA's
/// libraries missing or of another version) ran the model on the processor
/// without a word. And one that could load it and then found no usable
/// card (none visible, the driver's module not loaded after an update)
/// failed every load of the model with CUDA's message, where the processor
/// would have done.
fn load_with<S>(
    gpu_provider: bool,
    mut open: impl FnMut(bool) -> Result<S, String>,
) -> Result<(S, RunsOn), String> {
    let mut gpu_refused = None;
    if gpu_provider {
        match open(true) {
            Ok(session) => return Ok((session, RunsOn::Gpu)),
            Err(e) => gpu_refused = Some(e),
        }
    }
    Ok((open(false)?, RunsOn::Processor { gpu_refused }))
}

/// Run the model, and when it fails on the GPU, once more on the processor
/// with a session opened for it (`reopen`), which then stays. A GPU can
/// fail long after the model was loaded there: its memory taken by a game,
/// the card gone, a cuDNN the provider opens only at the first picture.
/// Every map failed from then on, a live screen went flat, and the
/// processor was never tried.
fn run_or_fall_back<S, R>(
    session: &mut S,
    runs_on: &mut RunsOn,
    run: impl Fn(&mut S, &RunsOn) -> Result<R, String>,
    reopen: impl FnOnce() -> Result<S, String>,
) -> Result<R, String> {
    let failed = match run(session, runs_on) {
        Err(e) if *runs_on == RunsOn::Gpu => e,
        done => return done,
    };
    // No processor either: the first failure is the one to tell.
    let Ok(on_processor) = reopen() else {
        return Err(failed);
    };
    *session = on_processor;
    *runs_on = RunsOn::Processor {
        gpu_refused: Some(failed),
    };
    run(session, runs_on)
}

pub struct Estimator {
    session: Session,
    input: String,
    model: PathBuf,
    runs_on: RunsOn,
    /// The pixels of the picture the model was last given.
    last_pixels: u64,
    /// The runtime takes the request to give room back (`GIVE_BACK`).
    gives_back: bool,
}

/// The one model, shared by still images and live sessions.
static ESTIMATOR: Mutex<Option<Estimator>> = Mutex::new(None);

/// Let the model go, with everything the runtime holds for it; the next
/// map loads it again. A live screen that has ended does this: its model
/// held 0.9 GB of a GPU's memory at standard detail and 2.8 GB at the
/// finest for as long as the program stayed open, output or none. Waits
/// for a map that is being made: not for the UI's thread.
pub fn unload() {
    *ESTIMATOR.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Share one model between still images and live sessions. A stopped live
/// session must not start expensive work after waiting for another caller.
/// Cancellation does not interrupt an inference already inside the runtime.
pub fn estimate_unless_cancelled(
    image: &RgbaImage,
    long_side: u32,
    purpose: Purpose,
    cancelled: impl Fn() -> bool,
) -> Result<Option<DepthMap>, String> {
    with_model(
        &ESTIMATOR,
        cancelled,
        || {
            let (model, origin) = locate_model()?;
            verify_installed(&model, origin, MODEL_SHA256)?;
            Estimator::load(&model)
        },
        |estimator| estimator.estimate(image, long_side, purpose),
    )
}

fn with_model<T, R>(
    model: &Mutex<Option<T>>,
    cancelled: impl Fn() -> bool,
    load: impl FnOnce() -> Result<T, String>,
    run: impl FnOnce(&mut T) -> Result<R, String>,
) -> Result<Option<R>, String> {
    if cancelled() {
        return Ok(None);
    }
    let mut slot = model
        .lock()
        .map_err(|_| "The depth model is not usable after an earlier failure.")?;
    // The caller may have stopped while another session held the model.
    if cancelled() {
        return Ok(None);
    }
    if slot.is_none() {
        *slot = Some(load()?);
    }
    // Initializing the runtime/model can itself take several seconds. Keep
    // the loaded model for the next caller, but skip this obsolete inference.
    if cancelled() {
        return Ok(None);
    }
    run(slot.as_mut().expect("loaded above")).map(Some)
}

/// Whether the installed runtime was built with the CUDA provider. That
/// says nothing yet about whether the provider can be loaded or finds a
/// card: only that this is a runtime installed for a GPU.
fn has_gpu_provider() -> bool {
    use ort::ep::ExecutionProvider as _;
    ort::ep::CUDA::is_available().unwrap_or(false)
}

/// A session for the model, on the GPU through the CUDA provider if `gpu`,
/// where every failure of the provider is an error.
fn open_session(model: &Path, gpu: bool) -> Result<Session, String> {
    let said = |e: &dyn std::fmt::Display| without_trace(&e.to_string()).to_owned();
    // On the processor the model is given a few cores, not all of them:
    // it runs beside whatever shows its result (a live screen's
    // renderer, Leia's eye tracker under Wine), and a model that takes
    // every core makes all of those late. On a GPU the setting is moot.
    let mut builder = Session::builder(runtime()?)
        .map_err(|e| said(&e))?
        .with_intra_threads(model_threads())
        .map_err(|e| said(&e))?
        // Idle model threads sleep instead of spinning on their cores.
        .with_intra_op_spinning(false)
        .map_err(|e| said(&e))?;
    if gpu {
        builder = builder
            .with_execution_providers([ort::ep::CUDA::default().build().error_on_failure()])
            .map_err(|e| said(&e))?;
    }
    builder.commit_from_file(model).map_err(|e| said(&e))
}

/// The run setting that has the runtime give back, when the run is over,
/// the room it took for it and no longer uses.
const GIVE_BACK: &str = "memory.enable_memory_arena_shrinkage";

impl Estimator {
    pub fn load(model: &Path) -> Result<Estimator, String> {
        // A runtime that is missing is not a GPU that refused.
        runtime()?;
        let (session, runs_on) = load_with(has_gpu_provider(), |gpu| open_session(model, gpu))
            .map_err(|e| format!("Could not load the depth model {}: {e}", model.display()))?;
        let input = session
            .inputs()
            .first()
            .map(|input| input.name().to_owned())
            .ok_or("The depth model has no input.")?;
        if session.outputs().is_empty() {
            return Err("The depth model has no output.".into());
        }
        note_runs_on(&runs_on);
        Ok(Estimator {
            session,
            input,
            model: model.to_owned(),
            runs_on,
            last_pixels: 0,
            gives_back: true,
        })
    }

    /// Estimate the nearness of every pixel of `image`, which the model is
    /// shown with its long side at about `long_side` pixels (one of
    /// [`DETAIL`]). The map comes back at that working size; the renderer
    /// stretches it to the image.
    pub fn estimate(
        &mut self,
        image: &RgbaImage,
        long_side: u32,
        purpose: Purpose,
    ) -> Result<DepthMap, String> {
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 {
            return Err("The source contains no image.".into());
        }
        let (w, h) = model_size(width, height, long_side);
        // The runtime keeps the room of the largest picture it has worked
        // on, for the next one: on a GPU 0.9 GB of the card's memory at
        // standard detail, 2.8 GB at the finest and 6.9 GB after one square
        // picture at the finest, until the program ended. It is given back
        // after a still image, which is not followed by another, and after
        // the first frame of a live screen that is smaller than the one
        // before (a coarser depth detail, a smaller screen). Not after
        // every frame: taking the room anew for each costs half as much
        // time again as the model itself.
        let pixels = u64::from(w) * u64::from(h);
        let give_back = self.gives_back && (purpose == Purpose::Still || pixels < self.last_pixels);
        self.last_pixels = pixels;
        let input = self.input.clone();
        let model = self.model.clone();
        let was = self.runs_on.clone();
        let not_taken = std::cell::Cell::new(false);
        let map = run_or_fall_back(
            &mut self.session,
            &mut self.runs_on,
            |session, runs_on| {
                // The room is the GPU's where the model runs there.
                let room = give_back.then_some(match runs_on {
                    RunsOn::Gpu => "gpu:0",
                    RunsOn::Processor { .. } => "cpu:0",
                });
                match run(session, &input, image, (w, h), room) {
                    // A runtime built without that kind of room refuses the
                    // request before it runs anything. The map matters
                    // more: it is made without the request, and when that
                    // works the request is not made again.
                    Err(_) if room.is_some() => {
                        let map = run(session, &input, image, (w, h), None);
                        not_taken.set(map.is_ok());
                        map
                    }
                    done => done,
                }
            },
            || open_session(&model, false),
        );
        if not_taken.get() {
            self.gives_back = false;
        }
        if self.runs_on != was {
            note_runs_on(&self.runs_on);
        }
        map
    }
}

/// One run of the model on `image` at a size (width, height), after which
/// the runtime gives back what it no longer uses of `give_back` ("gpu:0",
/// "cpu:0"), if that is named.
fn run(
    session: &mut Session,
    input: &str,
    image: &RgbaImage,
    (w, h): (u32, u32),
    give_back: Option<&str>,
) -> Result<DepthMap, String> {
    let tensor = Tensor::from_array((
        [1usize, 3, h as usize, w as usize],
        planar_input(image, w, h),
    ))
    .map_err(|e| format!("Could not prepare the image for the depth model: {e}"))?;
    let failed = |e: &dyn std::fmt::Display| {
        format!("The depth model failed: {}", without_trace(&e.to_string()))
    };
    let mut options = ort::session::RunOptions::new().map_err(|e| failed(&e))?;
    if let Some(room) = give_back {
        options.set(GIVE_BACK, room).map_err(|e| failed(&e))?;
    }
    let outputs = session
        .run_with_options(ort::inputs![input => tensor], &options)
        .map_err(|e| failed(&e))?;
    let (shape, values) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("The depth model returned an unexpected result: {e}"))?;
    // [1, H, W] or [1, 1, H, W]: the last two dimensions are the map.
    let dims: Vec<i64> = shape.iter().copied().collect();
    let (out_h, out_w) = match dims.as_slice() {
        [.., h, w] if *h > 0 && *w > 0 => (*h as u32, *w as u32),
        _ => return Err(format!("The depth model returned shape {dims:?}.")),
    };
    // Depth Anything's output grows with nearness, which is what the
    // renderer's map means too.
    DepthMap::normalized(out_w, out_h, values.to_vec()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A library as the linker writes one for a 64-bit little-endian
    /// processor: the header, two program headers, the 4000 bytes the
    /// second one loads, and a table of three sections at the end. Without
    /// `sections` it is a stripped one, which ends with its last segment.
    #[cfg(all(target_pointer_width = "64", target_endian = "little"))]
    fn library(sections: bool) -> Vec<u8> {
        let mut file = vec![0u8; 64 + 2 * 56 + 4000];
        file[..8].copy_from_slice(b"\x7fELF\x02\x01\x01\x00");
        let mut put = |at: usize, value: u64, width: usize| {
            file[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
        };
        put(16, 3, 2); // a shared object
        put(32, 64, 8); // the program headers follow the header
        put(52, 64, 2);
        put(54, 56, 2);
        put(56, 2, 2);
        if sections {
            put(40, 4176, 8);
            put(58, 64, 2);
            put(60, 3, 2);
        }
        // Both are loaded: the headers themselves, and the 4000 bytes.
        for (header, offset, length) in [(64, 0, 176), (120, 176, 4000)] {
            put(header, 1, 4);
            put(header + 8, offset, 8);
            put(header + 32, length, 8);
        }
        if sections {
            file.extend([0u8; 3 * 64]);
        }
        file
    }

    #[cfg(all(target_pointer_width = "64", target_endian = "little"))]
    #[test]
    fn a_library_that_is_cut_short_is_told_from_a_whole_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("libonnxruntime.so");
        let cut = |bytes: &[u8]| {
            std::fs::write(&path, bytes).unwrap();
            cut_short(&path)
        };
        let whole = library(true);
        assert_eq!(whole.len(), 4368);
        assert_eq!(cut(&whole), None);
        // More than its headers describe is not this function's business.
        assert_eq!(cut(&[whole.clone(), vec![0; 100]].concat()), None);
        // Cut in the table of sections, before it, in the segment, in the
        // program headers and in the header.
        for length in [4367, 4176, 2000, 176, 100, 64] {
            assert_eq!(
                cut(&whole[..length]),
                Some((length as u64, 4368)),
                "{length}"
            );
        }
        assert_eq!(cut(&whole[..40]), Some((40, 64)));
        // A stripped library has no table of sections: its segments decide.
        let stripped = library(false);
        assert_eq!(stripped.len(), 4176);
        assert_eq!(cut(&stripped), None);
        assert_eq!(cut(&stripped[..4175]), Some((4175, 4176)));
        assert_eq!(cut(&stripped[..200]), Some((200, 4176)));
        // What is not a library for this processor is the loader's to refuse,
        // which it does with a message: text, nothing, a 32-bit library, one
        // of the other byte order.
        assert_eq!(cut(b"this is not a library"), None);
        assert_eq!(cut(b""), None);
        assert_eq!(cut(b"\x7fELF"), None);
        for (at, value) in [(4, 1), (5, 2)] {
            let mut other = whole[..2000].to_vec();
            other[at] = value;
            assert_eq!(cut(&other), None);
        }
        assert_eq!(cut_short(dir.path()), None);
        assert_eq!(cut_short(&dir.path().join("missing")), None);
        // A real one: this test program is whole, and its first megabyte
        // is not.
        let program = std::env::current_exe().unwrap();
        assert_eq!(cut_short(&program), None);
        let mut start = vec![0u8; 1 << 20];
        std::io::Read::read_exact(&mut std::fs::File::open(&program).unwrap(), &mut start).unwrap();
        let length = std::fs::metadata(&program).unwrap().len();
        assert!(length > 1 << 20);
        assert!(matches!(cut(&start), Some((have, needed)) if have == 1 << 20 && needed <= length));
    }

    #[cfg(all(target_pointer_width = "64", target_endian = "little"))]
    #[test]
    fn a_cut_library_is_refused_with_its_name_wherever_the_loader_could_find_it() {
        let dir = tempfile::tempdir().unwrap();
        let (good, bad, empty) = (
            dir.path().join("good"),
            dir.path().join("bad"),
            dir.path().join("empty"),
        );
        for folder in [&good, &bad, &empty] {
            std::fs::create_dir(folder).unwrap();
        }
        let whole = library(true);
        std::fs::write(good.join("libonnxruntime.so"), &whole).unwrap();
        std::fs::write(bad.join("libonnxruntime.so"), &whole[..3000]).unwrap();
        // Named with its folder (ORT_DYLIB_PATH): that file and no other.
        let message =
            unloadable(&bad.join("libonnxruntime.so"), std::slice::from_ref(&good)).unwrap();
        assert!(
            message.contains(&bad.join("libonnxruntime.so").display().to_string())
                && message.contains("cut short")
                && message.contains("3000 bytes")
                && message.contains("4368")
                && !message.contains("ORT_DYLIB_PATH"),
            "{message}"
        );
        assert_eq!(
            unloadable(&good.join("libonnxruntime.so"), std::slice::from_ref(&bad)),
            None
        );
        // Named without one: wherever the loader looks.
        let name = Path::new(RUNTIME_LIBRARY);
        let missing = dir.path().join("missing");
        assert_eq!(
            unloadable(name, &[missing.clone(), empty.clone(), good.clone()]),
            None
        );
        assert_eq!(unloadable(name, &[]), None);
        let message = unloadable(name, &[missing, empty, good, bad.clone()]).unwrap();
        assert!(
            message.contains(&bad.join("libonnxruntime.so").display().to_string())
                && message.contains("ORT_DYLIB_PATH"),
            "{message}"
        );
        // A link to the library, as the distributions install it.
        std::os::unix::fs::symlink(bad.join("libonnxruntime.so"), bad.join("link.so")).unwrap();
        assert!(unloadable(&bad.join("link.so"), &[]).is_some());
        // The loader's own folders are absolute, whatever the variable holds.
        assert!(loader_folders().iter().all(|folder| folder.is_absolute()));
        assert!(loader_folders().contains(&PathBuf::from("/usr/lib")));
    }

    #[test]
    fn the_binding_accepts_the_runtimes_distributions_package() {
        // Built for the runtime interface of 1.22 the program refused the
        // ONNX Runtime 1.21 of Debian 13 and Ubuntu 25.10, and the binding
        // then also set an automatic choice of provider on every session,
        // over which Microsoft's own 1.22.0 ends the whole program. 17 is the
        // oldest interface the binding has (the `api-17` feature).
        assert_eq!(ort::MINOR_VERSION, 17);
    }

    #[test]
    fn the_gpu_is_asked_for_first_and_the_processor_takes_over_with_the_reason() {
        use std::cell::RefCell;
        let asked = RefCell::new(Vec::new());
        let open = |works: [bool; 2]| {
            let asked = &asked;
            move |gpu: bool| {
                asked.borrow_mut().push(gpu);
                if works[usize::from(gpu)] {
                    Ok(if gpu {
                        "gpu session"
                    } else {
                        "processor session"
                    })
                } else if gpu {
                    Err("libcudnn.so.9: cannot open shared object file".to_owned())
                } else {
                    Err("the model is damaged".to_owned())
                }
            }
        };
        let asked_for = || std::mem::take(&mut *asked.borrow_mut());
        // A runtime without the provider is never asked for the GPU, and
        // has nothing to say about one.
        assert_eq!(
            load_with(false, open([true, true])),
            Ok(("processor session", RunsOn::Processor { gpu_refused: None }))
        );
        assert_eq!(asked_for(), [false]);
        // One with the provider: the GPU, and nothing else is opened.
        assert_eq!(
            load_with(true, open([true, true])),
            Ok(("gpu session", RunsOn::Gpu))
        );
        assert_eq!(asked_for(), [true]);
        // The provider cannot start: the processor, with what was said.
        assert_eq!(
            load_with(true, open([true, false])),
            Ok((
                "processor session",
                RunsOn::Processor {
                    gpu_refused: Some("libcudnn.so.9: cannot open shared object file".into())
                }
            ))
        );
        assert_eq!(asked_for(), [true, false]);
        // Neither: the processor's failure is the one that is told.
        assert_eq!(
            load_with(true, open([false, false])),
            Err("the model is damaged".to_owned())
        );
        assert_eq!(
            load_with(false, open([false, true])),
            Err("the model is damaged".to_owned())
        );
    }

    #[test]
    fn a_gpu_that_fails_after_the_model_was_loaded_is_left_for_the_processor() {
        use std::cell::Cell;
        // A session is the place it runs at; the GPU's fails every run.
        let run = |session: &mut &str, on: &RunsOn| {
            assert_eq!(*on == RunsOn::Gpu, *session == "gpu");
            match *session {
                "gpu" => Err("CUDA failure 2: out of memory".to_owned()),
                "broken" => Err("the model failed".to_owned()),
                place => Ok(format!("a map from the {place}")),
            }
        };
        let reopened = Cell::new(0);
        let reopen = |to: Result<&'static str, String>| {
            let reopened = &reopened;
            move || {
                reopened.set(reopened.get() + 1);
                to
            }
        };
        let (mut session, mut runs_on) = ("gpu", RunsOn::Gpu);
        assert_eq!(
            run_or_fall_back(&mut session, &mut runs_on, run, reopen(Ok("processor"))),
            Ok("a map from the processor".to_owned())
        );
        assert_eq!(session, "processor");
        assert_eq!(
            runs_on,
            RunsOn::Processor {
                gpu_refused: Some("CUDA failure 2: out of memory".into())
            }
        );
        assert_eq!(reopened.get(), 1);
        // From then on the processor's session is the one that is run, and
        // a failure there is told as it is, with nothing opened anew.
        assert_eq!(
            run_or_fall_back(&mut session, &mut runs_on, run, reopen(Ok("another"))),
            Ok("a map from the processor".to_owned())
        );
        let (mut session, mut runs_on) = ("broken", RunsOn::Processor { gpu_refused: None });
        assert_eq!(
            run_or_fall_back(&mut session, &mut runs_on, run, reopen(Ok("another"))),
            Err("the model failed".to_owned())
        );
        assert_eq!((session, reopened.get()), ("broken", 1));
        // No processor either: the GPU's failure is told, and the GPU's
        // session stays for the next try.
        let (mut session, mut runs_on) = ("gpu", RunsOn::Gpu);
        assert_eq!(
            run_or_fall_back(
                &mut session,
                &mut runs_on,
                run,
                reopen(Err("no runtime".into()))
            ),
            Err("CUDA failure 2: out of memory".to_owned())
        );
        assert_eq!((session, &runs_on), ("gpu", &RunsOn::Gpu));
        // And one that works is run once, where it is.
        let runs = Cell::new(0);
        let (mut session, mut runs_on) = ("gpu", RunsOn::Gpu);
        let counted = |_: &mut &str, _: &RunsOn| {
            runs.set(runs.get() + 1);
            Ok::<_, String>(7)
        };
        assert_eq!(
            run_or_fall_back(&mut session, &mut runs_on, counted, reopen(Ok("processor"))),
            Ok(7)
        );
        assert_eq!((session, runs.get(), reopened.get()), ("gpu", 1, 2));
    }

    #[test]
    fn what_the_runtime_said_is_told_without_its_trace_and_briefly_on_the_page() {
        // As ONNX Runtime 1.29 words it with no card to be seen (shortened
        // in the middle only).
        let no_card = "/usr/src/debug/onnxruntime/onnxruntime-cuda/onnxruntime/core/providers/cuda/cuda_call.cc:154 std::conditional_t<THRW, void, onnxruntime::common::Status> onnxruntime::CudaCall(ERRTYPE, const char*, const char*, SUCCTYPE, const char*, const char*, int) [with ERRTYPE = cudaError; bool THRW = true] CUDA failure 100: no CUDA-capable device is detected ; GPU=-1 ; hostname=box ; file=/usr/src/debug/onnxruntime/cuda_execution_provider.cc ; line=341 ; expr=cudaSetDevice(info_.device_id); \n\nStacktrace:\n[unknown] [0x76f202a8abca]\n[unknown] [0x76f202a8ed15]\n";
        let plain = without_trace(no_card);
        assert!(
            plain.ends_with("expr=cudaSetDevice(info_.device_id);"),
            "{plain}"
        );
        assert!(!plain.contains("Stacktrace") && !plain.contains("0x76f2"));
        assert_eq!(without_trace("nothing to cut"), "nothing to cut");
        assert_eq!(
            gist(no_card),
            "CUDA failure 100: no CUDA-capable device is detected"
        );
        // A library that is missing: the end of the message names it.
        let missing = "/onnxruntime_src/onnxruntime/core/session/provider_bridge_ort.cc:1778 onnxruntime::Provider& onnxruntime::ProviderLibrary::Get() [ONNXRuntimeError] : 1 : FAIL : Failed to load library libonnxruntime_providers_cuda.so with error: libcublasLt.so.12: cannot open shared object file: No such file or directory";
        assert_eq!(
            gist(missing),
            "Failed to load library libonnxruntime_providers_cuda.so with error: libcublasLt.so.12: cannot open shared object file: No such file or directory"
        );
        // Longer than the page has room for: its end, from a whole word on.
        let long_path = missing.replace(
            "libonnxruntime_providers_cuda.so",
            "/opt/a/folder/with/a/long/name/and/another/one/inside/it/lib/libonnxruntime_providers_cuda.so",
        );
        let short = gist(&long_path);
        assert!(
            short.starts_with('…') && short.chars().count() <= 161,
            "{short}"
        );
        assert!(
            short.ends_with(
                "libcublasLt.so.12: cannot open shared object file: No such file or directory"
            ),
            "{short}"
        );
        // The card's memory refused, as the runtime's own store says it.
        assert_eq!(
            gist("Non-zero status code returned while running Softmax node. Name:'/blocks.0/attn/Softmax' Status Message: /usr/src/debug/onnxruntime/core/framework/bfc_arena.cc:359 void* onnxruntime::BFCArena::AllocateRawInternal(size_t, bool, onnxruntime::Stream*) Available memory of 451936256 is smaller than requested bytes of 517156096"),
            "Available memory of 451936256 is smaller than requested bytes of 517156096"
        );
        // Cut between words, and between letters of more than one byte.
        let long = "é".repeat(100) + " " + &"ü".repeat(100);
        assert_eq!(gist(&long), format!("…{}", "ü".repeat(100)));
        assert_eq!(gist("short"), "short");

        let on_gpu = RunsOn::Gpu;
        assert_eq!(on_gpu.describe(), "Depth is computed on the NVIDIA GPU.");
        assert!(on_gpu.for_log().contains("NVIDIA GPU"));
        let plain = RunsOn::Processor { gpu_refused: None };
        assert_eq!(plain.describe(), "Depth is computed on the processor.");
        let refused = RunsOn::Processor {
            gpu_refused: Some(plain_text(no_card)),
        };
        let line = refused.describe();
        assert!(
            line.contains("on the processor")
                && line.contains(
                    "could not be used (CUDA failure 100: no CUDA-capable device is detected)"
                )
                && line.contains("View logs"),
            "{line}"
        );
        // The log has all of it.
        assert!(refused.for_log().contains("expr=cudaSetDevice"));
    }

    fn plain_text(message: &str) -> String {
        without_trace(message).to_owned()
    }

    /// The model itself, where it runs and what becomes of a GPU that
    /// fails. Needs the depth model and ONNX Runtime, and says more with a
    /// runtime that has the CUDA provider. Run by hand:
    /// `cargo test --release -p any2odyssey depth::tests::the_model -- --ignored --nocapture`
    #[test]
    #[ignore = "needs the depth model and ONNX Runtime"]
    fn the_model_runs_where_it_says_it_does() {
        let image = RgbaImage::from_fn(640, 360, |x, y| {
            image::Rgba([(x / 3) as u8, (y / 2) as u8, ((x + y) / 5) as u8, 255])
        });
        assert_eq!(
            runs_on(),
            None,
            "nothing is said before the model is loaded"
        );
        for (purpose, detail) in [
            (Purpose::Still, 518),
            (Purpose::Live, 770),
            (Purpose::Live, 518),
            (Purpose::Still, 518),
        ] {
            let started = std::time::Instant::now();
            let map = estimate_unless_cancelled(&image, detail, purpose, || false)
                .expect("the depth model")
                .expect("not cancelled");
            assert_eq!((map.width(), map.height()), model_size(640, 360, detail));
            println!(
                "{purpose:?} at {detail}: {:?}; {}",
                started.elapsed(),
                runs_on().expect("said once loaded").for_log()
            );
        }
        let said = runs_on().unwrap();
        if has_gpu_provider() {
            assert_ne!(said, RunsOn::Processor { gpu_refused: None });
        } else {
            assert_eq!(said, RunsOn::Processor { gpu_refused: None });
        }
    }

    #[test]
    fn stopped_requests_skip_model_loading_and_inference() {
        let model = Mutex::new(None::<u32>);
        let result: Result<Option<u32>, String> = with_model(
            &model,
            || true,
            || panic!("a stopped request must not load the model"),
            |_| panic!("a stopped request must not infer"),
        );
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn requests_stopped_while_queued_do_not_run_after_the_current_inference() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{mpsc, Arc};
        let model = Arc::new(Mutex::new(Some(42u32)));
        let stop = Arc::new(AtomicBool::new(false));
        let held = model.lock().unwrap();
        let (checked, waiting) = mpsc::sync_channel(1);
        let worker_model = model.clone();
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            let first = AtomicBool::new(true);
            with_model(
                &worker_model,
                || {
                    let stopped = worker_stop.load(Ordering::SeqCst);
                    if first.swap(false, Ordering::SeqCst) {
                        checked.send(()).unwrap();
                    }
                    stopped
                },
                || Ok(0),
                |model| Ok(*model),
            )
        });
        // The caller has checked cancellation once while the model is busy.
        let initial_check = waiting.recv_timeout(std::time::Duration::from_secs(5));
        stop.store(true, Ordering::SeqCst);
        drop(held);
        let result = worker.join().unwrap();
        assert!(initial_check.is_ok(), "request should check before waiting");
        assert_eq!(result.unwrap(), None);
        assert_eq!(
            with_model(
                &model,
                || false,
                || panic!("already loaded"),
                |model| Ok(*model + 1)
            )
            .unwrap(),
            Some(43)
        );
    }

    #[test]
    fn cancellation_during_loading_keeps_the_model_but_skips_inference() {
        use std::cell::Cell;
        let stop = Cell::new(false);
        let model = Mutex::new(None);
        let result = with_model(
            &model,
            || stop.get(),
            || {
                stop.set(true);
                Ok(42u32)
            },
            |model| Ok(*model),
        );
        assert_eq!(result.unwrap(), None);
        assert_eq!(
            with_model(
                &model,
                || false,
                || panic!("already loaded"),
                |model| Ok(*model)
            )
            .unwrap(),
            Some(42)
        );
    }

    #[test]
    fn the_model_size_keeps_the_aspect_ratio_in_whole_patches() {
        assert_eq!(model_size(518, 518, 518), (518, 518));
        assert_eq!(model_size(3840, 2160, 518), (518, 294));
        assert_eq!(model_size(1080, 1920, 518), (294, 518));
        // Finer detail is a larger picture of the same shape.
        assert_eq!(model_size(3840, 2160, 1022), (1022, 574));
        assert_eq!(model_size(3840, 2160, 1274), (1274, 714));
        // Sizes outside the choices are brought to the nearest end.
        assert_eq!(model_size(3840, 2160, 0), (518, 294));
        assert_eq!(model_size(3840, 2160, 99999), (1274, 714));
        for long_side in DETAIL.map(|(side, _)| side) {
            for (w, h) in [(1, 1), (7, 5000), (1920, 1080), (640, 480), (13, 13)] {
                let (mw, mh) = model_size(w, h, long_side);
                assert!(mw >= PATCH && mh >= PATCH, "{w}x{h} -> {mw}x{mh}");
                assert_eq!((mw % PATCH, mh % PATCH), (0, 0), "{w}x{h} -> {mw}x{mh}");
                assert!(mw.max(mh) <= long_side + PATCH, "{w}x{h} -> {mw}x{mh}");
            }
        }
    }

    #[test]
    fn the_input_is_planar_rgb_normalized_with_the_model_statistics() {
        let image = RgbaImage::from_pixel(28, 14, image::Rgba([255, 0, 124, 9]));
        let data = planar_input(&image, 28, 14);
        let plane = 28 * 14;
        assert_eq!(data.len(), plane * 3);
        let expected = |value: f32, channel: usize| (value / 255.0 - MEAN[channel]) / STD[channel];
        for (channel, value) in [(0, 255.0), (1, 0.0), (2, 124.0)] {
            let got = data[channel * plane + 5];
            assert!(
                (got - expected(value, channel)).abs() < 1e-5,
                "{channel}: {got}"
            );
        }
    }

    #[test]
    fn a_download_is_accepted_only_with_the_pinned_hash() {
        assert_eq!(MODEL_SHA256.len(), 64);
        assert!(MODEL_SHA256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert!(MODEL_URL.starts_with("https://"));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("model.part");
        std::fs::write(&file, b"not the model").unwrap();
        let error = verify_download(&file, MODEL_SHA256).unwrap_err();
        assert!(error.contains("not the expected model"), "{error}");
        assert!(verify_download(&dir.path().join("missing"), MODEL_SHA256).is_err());
    }

    #[cfg(unix)]
    fn download_command(body: &str) -> std::process::Command {
        let mut command = std::process::Command::new("sh");
        // Curl's arguments become shell positional parameters, not code.
        command.args(["-c", body, "fake-curl"]);
        command
    }

    #[cfg(unix)]
    #[test]
    fn failed_downloads_preserve_the_installed_model_and_remove_staging_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(MODEL_FILE);
        std::fs::write(&target, b"installed").unwrap();
        let expected = any2odyssey_core::engine::sha256_hex(&target).unwrap();
        for body in ["printf wrong", "printf partial; exit 1"] {
            assert!(install_model_in(dir.path(), download_command(body), &expected).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"installed");
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
        assert!(install_model_in(
            dir.path(),
            std::process::Command::new(dir.path().join("missing-curl")),
            &expected,
        )
        .is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_legacy_partial_symlink_cannot_redirect_a_download() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(MODEL_FILE);
        let victim = dir.path().join("unrelated");
        std::fs::write(&victim, b"preserve me").unwrap();
        std::fs::write(&target, b"model").unwrap();
        let expected = any2odyssey_core::engine::sha256_hex(&target).unwrap();
        std::os::unix::fs::symlink(&victim, dir.path().join(format!("{MODEL_FILE}.part"))).unwrap();
        install_model_in(dir.path(), download_command("printf model"), &expected).unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"preserve me");
        assert_eq!(std::fs::read(&target).unwrap(), b"model");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn overlapping_downloads_publish_only_their_own_verified_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(MODEL_FILE);
        std::fs::write(&target, b"model").unwrap();
        let expected = any2odyssey_core::engine::sha256_hex(&target).unwrap();
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    barrier.wait();
                    install_model_in(
                        dir.path(),
                        download_command("printf mo; sleep 0.1; printf del"),
                        &expected,
                    )
                    .unwrap();
                });
            }
        });
        assert_eq!(std::fs::read(&target).unwrap(), b"model");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn the_override_is_the_first_place_a_model_is_looked_for() {
        let candidates = model_candidates();
        assert!(candidates.len() >= 3);
        assert!(candidates.iter().all(|(path, _)| path
            .file_name()
            .is_some_and(|name| name == MODEL_FILE)
            || std::env::var_os(MODEL_ENV).is_some()));
        let (last, origin) = candidates.last().unwrap();
        assert!(last.starts_with("/usr/share/any2odyssey"));
        assert_eq!(*origin, Origin::System);
        // Exactly one place is held to the pinned hash: the data folder.
        let data: Vec<_> = candidates
            .iter()
            .filter(|(_, origin)| *origin == Origin::Data)
            .collect();
        assert_eq!(data.len(), 1);
        assert!(data[0].0.ends_with(Path::new("models").join(MODEL_FILE)));
        let overrides = candidates
            .iter()
            .filter(|(_, origin)| *origin == Origin::Override)
            .count();
        assert_eq!(
            overrides,
            usize::from(std::env::var_os(MODEL_ENV).is_some_and(|p| !p.is_empty()))
        );
    }

    #[test]
    fn the_installed_model_is_checked_once_before_it_is_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join(MODEL_FILE);
        std::fs::write(&model, b"the model").unwrap();
        let expected = any2odyssey_core::engine::sha256_hex(&model).unwrap();
        let written = std::fs::metadata(&model).unwrap().modified().unwrap();
        let verified = Mutex::new(None);
        let check =
            |origin, expected: &str| verify_installed_with(&model, origin, expected, &verified);
        assert_eq!(check(Origin::Data, &expected), Ok(()));
        assert_eq!(check(Origin::Data, &expected.to_uppercase()), Ok(()));

        // Remembered by size and time: the file is not read a second time.
        // (Same length, and the time put back, to show it.)
        std::fs::write(&model, b"THE MODEL").unwrap();
        let file = std::fs::File::options().write(true).open(&model).unwrap();
        file.set_modified(written).unwrap();
        drop(file);
        assert_eq!(check(Origin::Data, &expected), Ok(()));

        // A file that has changed is read again, and refused with what to do.
        let file = std::fs::File::options().write(true).open(&model).unwrap();
        file.set_modified(written + std::time::Duration::from_secs(2))
            .unwrap();
        drop(file);
        let error = check(Origin::Data, &expected).unwrap_err();
        assert!(error.contains("--install-depth-model"), "{error}");
        assert!(
            error.contains(MODEL_ENV) && error.contains(&expected),
            "{error}"
        );
        // So is one cut short, and the refusal is remembered like the rest.
        std::fs::write(&model, b"the mod").unwrap();
        assert!(check(Origin::Data, &expected).is_err());
        assert!(check(Origin::Data, &expected).is_err());

        // A model of one's own and a packaged one are not held to the hash,
        // and not read for it.
        assert_eq!(check(Origin::Override, &expected), Ok(()));
        assert_eq!(check(Origin::System, &expected), Ok(()));
        let missing = dir.path().join("missing.onnx");
        for origin in [Origin::Override, Origin::System] {
            assert_eq!(
                verify_installed_with(&missing, origin, &expected, &verified),
                Ok(())
            );
        }
        let error =
            verify_installed_with(&missing, Origin::Data, &expected, &verified).unwrap_err();
        assert!(
            error.starts_with("Could not read the depth model"),
            "{error}"
        );
    }

    #[test]
    fn curl_ignores_its_configuration_file_and_cannot_stall_or_leave_https() {
        let arguments = download_arguments();
        assert_eq!(arguments[0], "-q", "only the first argument is honoured");
        let value = |option: &str| {
            let at = arguments.iter().position(|a| a == option);
            at.map(|at| arguments[at + 1].as_str())
        };
        assert_eq!(value("--proto"), Some("=https"));
        assert_eq!(value("--proto-redir"), Some("=https"));
        assert_eq!(value("--max-filesize"), Some("209715200"));
        assert_eq!(value("--connect-timeout"), Some("20"));
        assert_eq!(value("--speed-limit"), Some("1024"));
        assert_eq!(value("--speed-time"), Some("60"));
        assert_eq!(value("--output"), Some("-"));
        assert!(arguments.contains(&"--fail".to_owned()));
        assert!(arguments.contains(&"--location".to_owned()));
        // The address names one commit, not whatever is newest.
        let revision = MODEL_URL
            .split_once("/resolve/")
            .and_then(|(_, rest)| rest.split('/').next())
            .unwrap();
        assert_eq!(revision.len(), 40);
        assert!(revision.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[cfg(unix)]
    #[test]
    fn the_download_command_gets_the_arguments_before_the_address() {
        let dir = tempfile::tempdir().unwrap();
        let seen = dir.path().join("arguments");
        let models = dir.path().join("models");
        let mut command = download_command("printf '%s\\n' \"$@\" > \"$SEEN\"; printf model");
        command.env("SEEN", &seen);
        std::fs::create_dir(&models).unwrap();
        std::fs::write(models.join(MODEL_FILE), b"model").unwrap();
        let expected = any2odyssey_core::engine::sha256_hex(&models.join(MODEL_FILE)).unwrap();
        install_model_in(&models, command, &expected).unwrap();
        let mut wanted = download_arguments();
        wanted.push(MODEL_URL.to_owned());
        let seen = std::fs::read_to_string(seen).unwrap();
        assert_eq!(seen.lines().collect::<Vec<_>>(), wanted);
    }

    #[test]
    fn only_old_downloads_of_ours_are_cleared_away() {
        let dir = tempfile::tempdir().unwrap();
        let day = std::time::Duration::from_secs(24 * 60 * 60);
        let old = std::time::SystemTime::now() - day * 3;
        let aged = |name: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, b"partial").unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(old).unwrap();
            path
        };
        let stale = [
            aged(".depth-download-Ab3xQ9"),
            aged(&format!("{MODEL_FILE}.part")),
        ];
        // Another installer's download in progress: it holds the lock.
        let running = dir.path().join(".depth-download-Zz81kL");
        std::fs::write(&running, b"partial").unwrap();
        let writing = std::fs::File::options().write(true).open(&running).unwrap();
        assert!(matches!(crate::platform::try_lock_file(&writing), Ok(true)));
        // Old, but not ours to remove.
        let kept = [
            aged(MODEL_FILE),
            aged("notes.txt"),
            aged("depth-download-visible"),
            aged(&format!("other-{MODEL_FILE}.part")),
        ];
        let folder = dir.path().join(".depth-download-folder");
        std::fs::create_dir(&folder).unwrap();
        #[cfg(unix)]
        let (link, victim) = {
            let victim = aged("victim");
            let link = dir.path().join(".depth-download-link");
            std::os::unix::fs::symlink(&victim, &link).unwrap();
            (link, victim)
        };
        remove_stale_downloads(dir.path(), day);
        for path in &stale {
            assert!(!path.exists(), "{path:?}");
        }
        assert!(running.exists() && folder.is_dir());
        for path in &kept {
            assert!(path.exists(), "{path:?}");
        }
        #[cfg(unix)]
        assert!(link.is_symlink() && victim.exists());
        // A folder that is not there is nothing to clear.
        remove_stale_downloads(&dir.path().join("missing"), day);
        // Given up now (its installer is gone): cleared without waiting.
        drop(writing);
        assert!(cleared(dir.path(), &running));
    }

    /// Whether clearing `folder` up removes the download at `path`, whose
    /// lock was let go of just now. Tried for a moment: a program that
    /// another test's thread is starting holds every open file of this
    /// process, this lock with it, until it has become that program.
    fn cleared(folder: &Path, path: &Path) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            remove_stale_downloads(folder, STALE_DOWNLOAD);
            if !path.exists() {
                return true;
            }
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The download a killed installer leaves: its file, no longer locked.
    fn abandoned_download(folder: &Path) -> PathBuf {
        let (partial, output) = start_download(folder).unwrap();
        let (file, path) = partial.keep().unwrap();
        std::fs::write(&path, b"half a model").unwrap();
        drop((file, output));
        path
    }

    #[cfg(unix)]
    #[test]
    fn an_interrupted_download_is_cleared_by_the_next_installer() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(MODEL_FILE);
        std::fs::write(&target, b"installed").unwrap();
        let expected = any2odyssey_core::engine::sha256_hex(&target).unwrap();
        let left = [
            abandoned_download(dir.path()),
            abandoned_download(dir.path()),
        ];
        assert!(left.iter().all(|path| path.is_file()));
        // Minutes later, not a day: the next attempt, here one that fails.
        assert!(install_model_in(dir.path(), download_command("printf wrong"), &expected).is_err());
        for path in &left {
            assert!(
                cleared(dir.path(), path),
                "{path:?} was left for another day"
            );
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read(&target).unwrap(), b"installed");
    }

    #[cfg(unix)]
    #[test]
    fn a_download_running_in_another_installer_is_not_cleared_and_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let go = elsewhere.path().join("go");
        let target = dir.path().join(MODEL_FILE);
        std::fs::write(&target, b"model").unwrap();
        let expected = any2odyssey_core::engine::sha256_hex(&target).unwrap();
        std::fs::remove_file(&target).unwrap();
        let downloads = || {
            std::fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| std::fs::read(path).is_ok_and(|bytes| bytes == b"mo"))
                .collect::<Vec<_>>()
        };
        // Halfway, until told to go on.
        let slow = format!(
            "printf mo; while [ ! -e '{}' ]; do sleep 0.02; done; printf del",
            go.display()
        );
        // Tells the stand-in to go on, also when an assertion below fails:
        // the scope waits for the installer's thread, which waits for the
        // stand-in, and a running download that was removed showed as a
        // test that never ends instead of one that fails.
        //
        // Tried until the file is there: with no descriptor to be had at
        // that moment (the other tests of a run under `ulimit -n 32` held
        // them all) the one attempt failed without a word, and the test
        // never ended after all.
        struct Go<'a>(&'a Path);
        impl Drop for Go<'_> {
            fn drop(&mut self) {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
                while std::fs::write(self.0, b"").is_err() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
        std::thread::scope(|scope| {
            let go_on = Go(&go);
            let running =
                scope.spawn(|| install_model_in(dir.path(), download_command(&slow), &expected));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while downloads().is_empty() {
                assert!(std::time::Instant::now() < deadline, "no download started");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let theirs = downloads();
            // Another installer comes and goes, and clears up as it does.
            assert!(
                install_model_in(dir.path(), download_command("printf wrong"), &expected).is_err()
            );
            remove_stale_downloads(dir.path(), STALE_DOWNLOAD);
            assert_eq!(downloads(), theirs, "a running download was removed");
            drop(go_on);
            assert_eq!(running.join().unwrap(), Ok(target.clone()));
        });
        assert_eq!(std::fs::read(&target).unwrap(), b"model");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// The lock is this installer's until the model has its name, not only
    /// while curl runs: when the file is checked against the hash and moved
    /// into place curl has ended, and to an installer clearing up in that
    /// moment a download without a lock is one that was given up.
    #[cfg(unix)]
    #[test]
    fn a_download_is_not_cleared_while_it_is_checked_and_put_in_place() {
        const SIZE: usize = 8 * 1024 * 1024;
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        // Large enough that checking it takes a moment.
        let sample = elsewhere.path().join("sample");
        std::fs::write(&sample, vec![0u8; SIZE]).unwrap();
        let expected = any2odyssey_core::engine::sha256_hex(&sample).unwrap();
        let target = dir.path().join(MODEL_FILE);
        let done = std::sync::atomic::AtomicBool::new(false);
        let installed = std::thread::scope(|scope| {
            // Other installers, clearing up all the time.
            scope.spawn(|| {
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    remove_stale_downloads(dir.path(), STALE_DOWNLOAD);
                }
            });
            let installed = install_model_in(
                dir.path(),
                download_command(&format!("head -c {SIZE} /dev/zero")),
                &expected,
            );
            done.store(true, std::sync::atomic::Ordering::Relaxed);
            installed
        });
        assert_eq!(installed, Ok(target.clone()));
        assert_eq!(std::fs::metadata(&target).unwrap().len(), SIZE as u64);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_download_is_kept_for_as_long_as_its_curl_runs_and_cleared_after() {
        // The installer was killed and its curl was not: curl holds the
        // lock through the handle it inherited and goes on writing.
        let dir = tempfile::tempdir().unwrap();
        let (partial, output) = start_download(dir.path()).unwrap();
        let (file, path) = partial.keep().unwrap();
        let mut curl = std::process::Command::new("sleep")
            .arg("600")
            .stdout(std::process::Stdio::from(output))
            .spawn()
            .unwrap();
        drop(file);
        remove_stale_downloads(dir.path(), STALE_DOWNLOAD);
        let kept = path.is_file();
        curl.kill().unwrap();
        curl.wait().unwrap();
        assert!(kept, "removed under a curl that was still writing");
        assert!(cleared(dir.path(), &path));
    }

    #[test]
    fn a_new_download_taken_for_an_abandoned_one_is_not_used() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = || {
            let file = tempfile::Builder::new()
                .prefix(DOWNLOAD_PREFIX)
                .tempfile_in(dir.path())
                .unwrap();
            let output = file.reopen().unwrap();
            (file, output)
        };
        // Nobody else: claimed, and then nobody else's to clear.
        let (file, output) = fresh();
        assert!(claim(&output, file.path()));
        remove_stale_downloads(dir.path(), STALE_DOWNLOAD);
        assert!(file.path().is_file());
        // An installer that is clearing up has it locked this moment.
        let (file, output) = fresh();
        let clearing = std::fs::File::open(file.path()).unwrap();
        assert!(matches!(
            crate::platform::try_lock_file(&clearing),
            Ok(true)
        ));
        assert!(!claim(&output, file.path()));
        drop(clearing);
        // Or has removed it already, which the open handle does not show.
        let (file, output) = fresh();
        remove_stale_downloads(dir.path(), STALE_DOWNLOAD);
        assert!(!file.path().exists());
        assert!(!claim(&output, file.path()));
    }
}
