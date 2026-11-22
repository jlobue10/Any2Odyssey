//! Depth from a single image, for the stereo viewer's 2D → 3D mode.
//!
//! The model is Depth Anything V2 (small) in ONNX form, run by the system's
//! ONNX Runtime. Neither ships inside the program: the runtime library is
//! opened and the model file read when the first depth map is asked for, and
//! a missing one is an ordinary error message with what to install.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use any2odyssey_core::dibr::{DepthMap, DibrError};
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
/// A runtime of the user's own in the data folder, used before the
/// system's when it is there: `libonnxruntime.so.1` with its provider
/// libraries, and TensorRT's beside them where it has that provider. No
/// distribution packages a runtime with the TensorRT provider so far.
const RUNTIME_FOLDER: &str = "depth-runtime";
const RUNTIME_IN_FOLDER: &str = "libonnxruntime.so.1";
/// Environment: the folder TensorRT's libraries (`libnvinfer.so.*`) are in,
/// where the system's loader would not find them by itself.
pub const TENSORRT_FOLDER_ENV: &str = "ANY2ODYSSEY_TENSORRT_DIR";
/// Environment: `0` leaves TensorRT unused where it could be used.
pub const TENSORRT_ENV: &str = "ANY2ODYSSEY_TENSORRT";
/// Where the forms TensorRT makes of the model are kept, in the data
/// folder: a folder for each size of picture.
const ENGINES_FOLDER: &str = "depth-engines";

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
    let plane = width as usize * height as usize;
    let mut data = vec![0.0f32; plane * 3];
    // A colour as the model wants it, for each of the 256 there are.
    let wanted: [[f32; 256]; 3] = std::array::from_fn(|channel| {
        std::array::from_fn(|colour| (colour as f32 / 255.0 - MEAN[channel]) / STD[channel])
    });
    // A live screen's frames come at the model's size already, or at twice
    // it (`live::model_input_size`): those are halved here.
    if image.dimensions() == (width * 2, height * 2) && plane > 0 {
        halved_planes(image, width as usize, height as usize, &wanted, &mut data);
        return data;
    }
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
    for (index, pixel) in resized.pixels().enumerate() {
        for channel in 0..3 {
            data[channel * plane + index] = wanted[channel][pixel.0[channel] as usize];
        }
    }
    data
}

/// `image`, which is twice `width` x `height`, halved into the three
/// planes of `data` with each colour as `wanted` has it.
///
/// The halving is the one `image::imageops::resize` does with its triangle
/// filter, which this used to call: every pixel is made of four columns
/// and four rows, weighted 1, 3, 3, 1 (three of them at an edge), so that
/// a line one pixel wide does not come and go as the screen moves under
/// it (`live::model_input_size` has the numbers). That call works on one
/// processor core and through numbers with a fraction: for a 3840 x 2160
/// screen at *Finest* (2548 x 1428 to 1274 x 714) it took 23.8 ms of the
/// depth thread before the model, which takes about as long, had seen the
/// frame, and a film's depth map came later than the film's next picture.
/// Here the same sums are made in whole numbers, in bands over a few
/// cores (`live::band_threads`).
fn halved_planes(
    image: &RgbaImage,
    width: usize,
    height: usize,
    wanted: &[[f32; 256]; 3],
    data: &mut [f32],
) {
    let (source_width, source_height) = (width * 2, height * 2);
    let source = image.as_raw();
    // The first of the rows (or columns) a halved one is made of, and
    // their weights: 1, 3, 3, 1 from one before its own two to one after,
    // without what would lie outside.
    let taps = |at: usize, side: usize| -> (usize, [u32; 4]) {
        let mut weights = [1, 3, 3, 1];
        if at == 0 {
            weights[0] = 0;
        }
        if at * 2 + 2 >= side {
            weights[3] = 0;
        }
        ((at * 2).saturating_sub(1), weights)
    };
    let threads = any2odyssey_core::live::band_threads(height);
    let band = height.div_ceil(threads);
    let (red, rest) = data.split_at_mut(width * height);
    let (green, blue) = rest.split_at_mut(width * height);
    let work = |first: usize, red: &mut [f32], green: &mut [f32], blue: &mut [f32]| {
        // One halved row's columns before they are halved: the weighted
        // sums of its rows, for each colour.
        let mut sums = vec![[0u32; 3]; source_width];
        let rows = red
            .chunks_exact_mut(width)
            .zip(green.chunks_exact_mut(width))
            .zip(blue.chunks_exact_mut(width));
        for (row, ((red, green), blue)) in rows.enumerate() {
            let (top, down) = taps(first + row, source_height);
            sums.fill([0; 3]);
            let mut skipped = 0;
            for (index, weight) in down.into_iter().enumerate() {
                if weight == 0 {
                    skipped += usize::from(index == 0 && top == 0);
                    continue;
                }
                let y = top + index - skipped;
                let line = &source[y * source_width * 4..(y + 1) * source_width * 4];
                for (sum, pixel) in sums.iter_mut().zip(line.as_chunks::<4>().0) {
                    sum[0] += weight * u32::from(pixel[0]);
                    sum[1] += weight * u32::from(pixel[1]);
                    sum[2] += weight * u32::from(pixel[2]);
                }
            }
            let down: u32 = down.iter().sum();
            for x in 0..width {
                let (left, across) = taps(x, source_width);
                let mut total = [0u32; 3];
                let mut skipped = 0;
                for (index, weight) in across.into_iter().enumerate() {
                    if weight == 0 {
                        skipped += usize::from(index == 0 && left == 0);
                        continue;
                    }
                    let sum = sums[left + index - skipped];
                    total[0] += weight * sum[0];
                    total[1] += weight * sum[1];
                    total[2] += weight * sum[2];
                }
                let all = down * across.iter().sum::<u32>();
                let colour = |channel: usize| ((total[channel] * 2 + all) / (all * 2)) as usize;
                red[x] = wanted[0][colour(0)];
                green[x] = wanted[1][colour(1)];
                blue[x] = wanted[2][colour(2)];
            }
        }
    };
    std::thread::scope(|scope| {
        let work = &work;
        let bands = red
            .chunks_mut(band * width)
            .zip(green.chunks_mut(band * width))
            .zip(blue.chunks_mut(band * width));
        for (index, ((red, green), blue)) in bands.enumerate() {
            if index + 1 == threads || (index + 1) * band >= height {
                // The last band is this thread's own.
                work(index * band, red, green, blue);
            } else {
                scope.spawn(move || work(index * band, red, green, blue));
            }
        }
    });
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

/// The runtime library to load: the one named in `ORT_DYLIB_PATH`, else the
/// user's own in the data folder ([`RUNTIME_FOLDER`]) when there is one,
/// else the system's.
fn runtime_library(named: Option<std::ffi::OsString>, data: &Path) -> PathBuf {
    if let Some(named) = named.filter(|p| !p.is_empty()) {
        return PathBuf::from(named);
    }
    let own = data.join(RUNTIME_FOLDER).join(RUNTIME_IN_FOLDER);
    if own.is_file() {
        own
    } else {
        PathBuf::from(RUNTIME_LIBRARY)
    }
}

/// TensorRT's libraries in `folder` that the runtime's TensorRT provider
/// is linked against, by name: the library itself before the one that
/// reads models for it and needs it.
fn tensorrt_libraries(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| {
                    ["libnvinfer.so.", "libnvonnxparser.so."]
                        .iter()
                        .any(|start| {
                            name.strip_prefix(start)
                                .is_some_and(|major| major.parse::<u32>().is_ok())
                        })
                })
        })
        .collect();
    found.sort();
    found
}

/// Make TensorRT's functions available to the runtime's TensorRT provider
/// where TensorRT is not installed for the whole system: from the folder
/// named in `ANY2ODYSSEY_TENSORRT_DIR`, and from the user's own runtime's
/// folder. The libraries find the rest of TensorRT beside themselves.
/// Done once, before the first use of the provider and not when the
/// runtime is started: the library has 660 MB. Fails, for as long as the
/// program runs, where one of TensorRT's libraries is cut short
/// ([`cut_tensorrt_library`]).
fn preload_tensorrt() -> Result<(), String> {
    use libloading::os::unix::{Library, RTLD_GLOBAL, RTLD_NOW};
    static DONE: OnceLock<Result<(), String>> = OnceLock::new();
    DONE.get_or_init(|| {
        let folders: Vec<PathBuf> = std::env::var_os(TENSORRT_FOLDER_ENV)
            .filter(|folder| !folder.is_empty())
            .map(PathBuf::from)
            .into_iter()
            .chain([paths::data_dir().join(RUNTIME_FOLDER)])
            .collect();
        preload_tensorrt_from(&folders, |path| {
            // Avoid handing an incomplete ELF file to the native loader.
            if !path.is_file() || cut_short(path).is_some() {
                return false;
            }
            // SAFETY: as for cuDNN, these are the configured provider libraries.
            if let Ok(library) = unsafe { Library::open(Some(path), RTLD_NOW | RTLD_GLOBAL) } {
                std::mem::forget(library);
                true
            } else {
                false
            }
        })
    })
    .clone()
}

fn preload_tensorrt_from(
    folders: &[PathBuf],
    mut open: impl FnMut(&Path) -> bool,
) -> Result<(), String> {
    // Before anything of TensorRT is opened: what is opened here brings
    // the rest of it in later, by itself.
    if let Some(refusal) = cut_tensorrt_library(folders) {
        return Err(refusal);
    }
    for folder in folders {
        let mut loaded = false;
        for path in tensorrt_libraries(folder) {
            loaded |= open(&path);
        }
        if loaded {
            break;
        }
    }
    Ok(())
}

/// What to say instead of using TensorRT, when one of its libraries in
/// `folders` is cut short ([`cut_short`]).
///
/// Only the two libraries the runtime's provider is linked against are
/// opened here. TensorRT opens the others by itself: its plugins with the
/// first session, and the part of its builder that is made for the GPU
/// (`libnvinfer_builder_resource_*`) when it makes its form of the model
/// for a size it has none for. Seen with the real libraries: with either
/// of those cut short the whole program ended with "Bus error" inside the
/// loader, the plugins' at the start of every live screen. And with one
/// of the two cut short beside the other whole, the provider said "cannot
/// open shared object file: No such file or directory" of a file that is
/// there. So every library of TensorRT's in the folders it may be taken
/// from is looked at first, and with one of them cut short TensorRT is
/// left alone and the file is named.
fn cut_tensorrt_library(folders: &[PathBuf]) -> Option<String> {
    let mut libraries: Vec<PathBuf> = folders
        .iter()
        .flat_map(|folder| std::fs::read_dir(folder).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    (name.starts_with("libnvinfer") || name.starts_with("libnvonnxparser"))
                        && name.contains(".so")
                })
        })
        .collect();
    libraries.sort();
    libraries.iter().find_map(|library| {
        let (length, needed) = cut_short(library)?;
        // What it is and the file last: the page shows the end of a long
        // message (`gist`).
        Some(format!(
            "one of TensorRT's libraries is not whole ({length} of {needed} bytes), as an interrupted copy or download leaves it. Copy it again, or run depth-runtime/setup.sh again. It is cut short: {}",
            library.display()
        ))
    })
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
            let library = runtime_library(std::env::var_os(RUNTIME_ENV), &paths::data_dir());
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
    /// On an NVIDIA GPU through the CUDA provider, while TensorRT makes
    /// its form of the model for a live screen's size ([`Fast`]).
    Preparing,
    /// On an NVIDIA GPU: a live screen's maps through the runtime's
    /// TensorRT provider, in half precision, a still image's as [`Gpu`].
    ///
    /// [`Gpu`]: RunsOn::Gpu
    TensorRt,
    /// On an NVIDIA GPU through the CUDA provider, because TensorRT, which
    /// the runtime has the provider for, could not be used.
    GpuOnly { tensorrt_refused: String },
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
            RunsOn::Preparing => "Depth is computed on the NVIDIA GPU. A faster form of the model is being made for this screen's size with TensorRT, once: it takes over in about half a minute.".into(),
            RunsOn::TensorRt => "Depth is computed on the NVIDIA GPU, a live screen's with TensorRT.".into(),
            RunsOn::GpuOnly { tensorrt_refused } => format!(
                "Depth is computed on the NVIDIA GPU, without TensorRT, which could not be used ({}). View logs has the whole message; TensorRT is tried again when the program is started again.",
                gist(tensorrt_refused)
            ),
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
            RunsOn::Preparing => "Depth model: runs on the NVIDIA GPU (ONNX Runtime's CUDA provider) while TensorRT makes its form of the model for this size of picture.".into(),
            RunsOn::TensorRt => "Depth model: a live screen's maps run on the NVIDIA GPU through ONNX Runtime's TensorRT provider, in half precision.".into(),
            RunsOn::GpuOnly { tensorrt_refused } => format!(
                "Depth model: runs on the NVIDIA GPU (ONNX Runtime's CUDA provider), because TensorRT could not be used. ONNX Runtime's TensorRT provider said: {tensorrt_refused}"
            ),
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

/// The model's answer as it gave it: a number for every pixel, larger for
/// nearer, on a scale of the model's own that is another for every picture.
///
/// A still image's map is brought to 0..=1 at once ([`RawDepth::normalized`]),
/// which is all there is to do with one picture. A live screen's is not:
/// its maps are steadied against each other, and which range each is
/// brought to 0..=1 with is for what steadies them to say
/// (`core::live::DepthSmoother`): its own, by what was measured with the
/// model, or one that follows the scene, as 1.0.1 had it. That choice needs
/// each map's own numbers, and a map that has been brought to 0..=1 has
/// none left; the live screen was given such maps until 1.0.0.
#[derive(Clone, Debug, PartialEq)]
pub struct RawDepth {
    width: u32,
    height: u32,
    values: Vec<f32>,
}

impl RawDepth {
    /// A map of `width` x `height` numbers, row by row. Refused, with what
    /// [`DepthMap::normalized`] says of it, when there is none, when the
    /// numbers are not as many as the size says, or when one of them is
    /// not a number; and when they lie further apart than a number can say.
    pub fn new(width: u32, height: u32, values: Vec<f32>) -> Result<RawDepth, String> {
        if width == 0 || height == 0 {
            return Err(DibrError::Empty.to_string());
        }
        if values.len() != width as usize * height as usize {
            return Err(DibrError::BadDepthMap.to_string());
        }
        let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
        for &v in &values {
            if !v.is_finite() {
                return Err(DibrError::NotFinite.to_string());
            }
            low = low.min(v);
            high = high.max(v);
        }
        if !(high - low).is_finite() {
            return Err("The depth map's values are too far apart to tell near from far.".into());
        }
        Ok(RawDepth {
            width,
            height,
            values,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Every number as the model gave it, row by row.
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// Put the map's outermost rows and columns, so many at the `top`, the
    /// `bottom`, the `left` and the `right`, where the farthest point of
    /// the rest is: what the model said of black bars around a picture
    /// (`live::black_bars`) then says nothing about how near the picture
    /// is. Nothing changes where no rows or columns are named, or none
    /// would be left.
    pub fn far_at_the_edges(&mut self, top: u32, bottom: u32, left: u32, right: u32) {
        let (w, h) = (self.width as usize, self.height as usize);
        let (top, bottom) = (top as usize, bottom as usize);
        let (left, right) = (left as usize, right as usize);
        if top + bottom + left + right == 0 || top + bottom >= h || left + right >= w {
            return;
        }
        let inside =
            |x: usize, y: usize| (top..h - bottom).contains(&y) && (left..w - right).contains(&x);
        let far = (top..h - bottom)
            .flat_map(|y| &self.values[y * w + left..(y + 1) * w - right])
            .fold(f32::INFINITY, |far, &v| far.min(v));
        for (index, value) in self.values.iter_mut().enumerate() {
            if !inside(index % w, index / w) {
                *value = far;
            }
        }
    }

    /// The map for one picture by itself: 0 its farthest point, 1 its
    /// nearest.
    pub fn normalized(self) -> Result<DepthMap, String> {
        DepthMap::normalized(self.width, self.height, self.values).map_err(|e| e.to_string())
    }
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

/// The model as TensorRT runs it, for a live screen: made for one size of
/// picture, in half precision, several times as fast as the CUDA provider
/// runs the model as it is (on an RTX 5090 a 3840 x 2160 screen's map at
/// the finest detail in 4 ms for 27, at the standard one in 0.8 ms for 3).
///
/// Making it takes TensorRT from ten seconds to half a minute, for every
/// size once: it is kept in the data folder and opened from there in a
/// fifth of a second. So it is made on a thread of its own while the maps
/// go on being made as before, and takes over when it is there. A still
/// image is never run this way: each has a size of its own, and waits for
/// nothing that half a minute could save.
enum Fast<S> {
    /// Not asked for yet.
    Unasked,
    /// Being made for pictures of `size` (width, height).
    Making {
        size: (u32, u32),
        made: std::sync::mpsc::Receiver<Result<S, String>>,
    },
    Ready {
        size: (u32, u32),
        session: S,
    },
    /// TensorRT could not make it, or could not run it, and said this. It
    /// is not asked again while the program runs.
    Refused(String),
}

impl<S: Send + 'static> Fast<S> {
    /// The session for pictures of `size`, when it is there. When none is
    /// there and none is being made for that size, `make` begins to make
    /// one on a thread of its own; a later call finds it.
    fn session_for(
        &mut self,
        size: (u32, u32),
        make: impl FnOnce() -> Result<S, String> + Send + 'static,
    ) -> Option<&mut S> {
        use std::sync::mpsc::{channel, TryRecvError};
        if let Fast::Making {
            size: building,
            made,
        } = self
        {
            match made.try_recv() {
                // Keep using CUDA until this build finishes, even if the
                // requested size changes. Dropping its receiver cannot stop
                // TensorRT's native build or release its GPU memory.
                Err(TryRecvError::Empty) => return None,
                // A refusal is TensorRT's, whatever size it was asked
                // for: it is told and not asked again, as it is for the
                // size that is wanted. Only what was made for another
                // size is let go of.
                Ok(Err(why)) => *self = Fast::Refused(why),
                Err(TryRecvError::Disconnected) => {
                    *self = Fast::Refused("the thread that made it ended without a word".into());
                }
                Ok(Ok(_)) if *building != size => *self = Fast::Unasked,
                Ok(Ok(session)) => *self = Fast::Ready { size, session },
            }
        }
        match self {
            Fast::Refused(_) => return None,
            Fast::Ready { size: its, .. } if *its == size => {}
            // No build is running. Start one for the size requested now.
            _ => {
                let (send, made) = channel();
                let begun = std::thread::Builder::new()
                    .name("depth-tensorrt".into())
                    .spawn(move || {
                        let _ = send.send(make());
                    });
                *self = match begun {
                    Ok(_) => Fast::Making { size, made },
                    Err(e) => Fast::Refused(format!("no thread could be started to make it: {e}")),
                };
                return None;
            }
        }
        match self {
            Fast::Ready { session, .. } => Some(session),
            _ => None,
        }
    }
}

/// How many of TensorRT's forms of the model are being made, or wait
/// their turn to be, in the whole program.
static MAKING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// `make`, to be run when no other such making is under way anywhere in
/// the program, and counted in [`MAKING`] while it waits for that and
/// while it runs.
///
/// [`Fast`] makes one form at a time, but each model has a `Fast` of its
/// own, and the model is let go of when the last live screen ends
/// (`unload_if`) while what is being made for it goes on being made:
/// TensorRT cannot be stopped. Seen with the real thing: output stopped
/// while a form was being made and started again made a second one beside
/// the first, for the same size and into the same file, each with the
/// GPU's memory of one (6 GB at the finest detail). Here the second waits
/// for the first, and then finds what the first has kept, which it opens
/// in a second.
fn one_at_a_time<S>(
    make: impl FnOnce() -> Result<S, String> + Send + 'static,
) -> impl FnOnce() -> Result<S, String> + Send + 'static {
    use std::sync::atomic::Ordering::SeqCst;
    static ONE: Mutex<()> = Mutex::new(());
    // Counted out when the making is over, however it ends.
    struct Counted;
    impl Drop for Counted {
        fn drop(&mut self) {
            MAKING.fetch_sub(1, SeqCst);
        }
    }
    move || {
        MAKING.fetch_add(1, SeqCst);
        let _counted = Counted;
        // (one that broke while it was its turn is no reason to stop)
        let _turn = ONE.lock().unwrap_or_else(|e| e.into_inner());
        make()
    }
}

/// End the program here and now, with `code`, if TensorRT is making a form
/// of the model at this moment; otherwise come back, for the program to
/// end as programs do.
///
/// A program that ends runs what its libraries have left to do at the
/// end, on the thread that ends it, while its other threads still run.
/// TensorRT's maker does not survive that: seen with the real thing, a
/// program that left while a form was being made ended with "Segmentation
/// fault" inside TensorRT, on the thread that made it. Nothing can stop
/// the maker and waiting for it takes up to a minute, so the program
/// ends without those last steps; what it has to put away itself (the
/// output, the capture, its files) is put away before it gets here.
pub fn leave_at_once_while_making(code: i32) {
    if MAKING.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        return;
    }
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    extern "C" {
        fn _exit(status: std::ffi::c_int) -> !;
    }
    // SAFETY: the C library's `_exit` takes any status and does not
    // return; nothing of this program runs after it.
    unsafe { _exit(code) }
}

pub struct Estimator {
    session: Session,
    /// The model as TensorRT runs it, for a live screen.
    fast: Fast<Session>,
    /// Whether that may be asked for: the runtime has the TensorRT
    /// provider, and `ANY2ODYSSEY_TENSORRT` does not say no.
    tensorrt: bool,
    /// What was last told of where the model runs.
    told: RunsOn,
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

/// Let the model go, with everything the runtime holds for it, if
/// `unused` says so once no map is being made; the next map loads it
/// again. A live screen that has ended does this: its model held 0.9 GB of
/// a GPU's memory at standard detail and 2.8 GB at the finest for as long
/// as the program stayed open, output or none. Waits for a map that is
/// being made: not for the UI's thread.
///
/// Whether the model is unused was decided before the wait and not asked
/// again after it. A live screen that began in between and was given the
/// model first had it let go of behind its first map, and loaded it once
/// more for its second: seconds, where a GPU is started for the first
/// time.
pub fn unload_if(unused: impl FnOnce() -> bool) {
    if unload_from(&ESTIMATOR, unused) {
        return_freed_memory();
    }
}

/// Whether a model was let go of.
fn unload_from<T>(model: &Mutex<Option<T>>, unused: impl FnOnce() -> bool) -> bool {
    let mut slot = model.lock().unwrap_or_else(|e| e.into_inner());
    // Whoever wants the model from here on waits for this, and loads it
    // after: nobody's model is taken from under them.
    if unused() {
        slot.take().is_some()
    } else {
        false
    }
}

/// Have the C library give back to the system what the model's sessions
/// took of the processor's memory and have freed.
///
/// The sessions are opened and let go of on threads that come and go with
/// each live screen, and glibc keeps what a thread has freed in a store of
/// that thread's kind, for the next thread: up to eight such stores for
/// every processor core, each with what the largest session left in it.
/// Measured with TensorRT's session (an RTX 5090, 32 cores, a live screen
/// started and stopped once a second, each time with the model loaded and
/// let go of): the program's own memory was 1.0 GB after the first stop,
/// 4.7 GB after ten, 9.1 GB after forty and 10 GB after sixty, with
/// nothing loaded; with the CUDA provider alone 0.9 GB became 1.8 GB.
/// Asked here, each time the model has been let go of, glibc gives it
/// back: 1.9 GB at the most over sixty such stops, 1.1 GB with the CUDA
/// provider alone.
fn return_freed_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        extern "C" {
            fn malloc_trim(pad: usize) -> std::ffi::c_int;
        }
        // SAFETY: glibc's own call, with nothing of ours in its hands; it
        // takes the allocator's locks as `free` does.
        unsafe {
            malloc_trim(0);
        }
    }
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
    estimate_raw_unless_cancelled(image, long_side, purpose, cancelled)?
        .map(RawDepth::normalized)
        .transpose()
}

/// [`estimate_unless_cancelled`] with the model's answer as it gave it,
/// for a live screen: its maps are brought to 0..=1 by what steadies them
/// ([`RawDepth`]).
pub fn estimate_raw_unless_cancelled(
    image: &RgbaImage,
    long_side: u32,
    purpose: Purpose,
    cancelled: impl Fn() -> bool,
) -> Result<Option<RawDepth>, String> {
    with_model(
        &ESTIMATOR,
        cancelled,
        || {
            let (model, origin) = locate_model()?;
            verify_installed(&model, origin, MODEL_SHA256)?;
            Estimator::load(&model)
        },
        |estimator| estimator.estimate_raw(image, long_side, purpose),
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

/// Whether the installed runtime was built with the TensorRT provider, and
/// the user has not said to leave it (`ANY2ODYSSEY_TENSORRT=0`).
fn may_use_tensorrt(said: Option<std::ffi::OsString>, provider: impl FnOnce() -> bool) -> bool {
    said.is_none_or(|said| said != "0") && provider()
}

fn has_tensorrt_provider() -> bool {
    use ort::ep::ExecutionProvider as _;
    ort::ep::TensorRT::is_available().unwrap_or(false)
}

/// The folder TensorRT's form of the model for pictures of `width` x
/// `height` is kept in.
fn engines_folder(data: &Path, (width, height): (u32, u32)) -> PathBuf {
    data.join(ENGINES_FOLDER).join(format!("{width}x{height}"))
}

/// Whether `folder` holds a form of the model TensorRT made earlier.
fn holds_an_engine(folder: &Path) -> bool {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.path().extension().is_some_and(|e| e == "engine"))
}

/// Open `folder`'s form of the model with `open`, which makes it where
/// there is none. Where one was kept and `open` fails, it is tried once
/// more without the kept one: TensorRT reads only what the same TensorRT
/// wrote, for the same kind of GPU, and after an update of either the kept
/// one would otherwise refuse TensorRT for good.
///
/// The kept one is cleared away only when making it anew works. It used
/// to be cleared before the second try, whatever had failed, and what
/// fails is hardly ever the kept one: ONNX Runtime 1.32 makes a form it
/// cannot read (cut short, empty, another TensorRT's) anew by itself and
/// says nothing. Seen with the real libraries: with TensorRT's own
/// library missing or cut short every start of a live screen threw away
/// the form of the model for the size in use, made in most of a minute,
/// and could make no other.
fn open_kept_or_anew<S>(
    folder: &Path,
    mut open: impl FnMut() -> Result<S, String>,
) -> Result<S, String> {
    let made = |folder: &Path| {
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("the folder {} could not be made: {e}", folder.display()))
    };
    let kept = holds_an_engine(folder);
    made(folder)?;
    let failed = match open() {
        Err(failed) if kept => failed,
        done => return done,
    };
    // Aside, beside the folder: "518x294" as "518x294.kept".
    let mut aside = folder.as_os_str().to_owned();
    aside.push(".kept");
    let aside = PathBuf::from(aside);
    let _ = std::fs::remove_dir_all(&aside);
    if std::fs::rename(folder, &aside).is_err() {
        // Nothing can be made where nothing can be moved.
        return Err(failed);
    }
    match made(folder).and_then(|()| open()) {
        Ok(session) => {
            let _ = std::fs::remove_dir_all(&aside);
            Ok(session)
        }
        Err(again) => {
            // Not the kept one's doing: it is there for the next start.
            let _ = std::fs::remove_dir_all(folder);
            let _ = std::fs::rename(&aside, folder);
            Err(again)
        }
    }
}

/// A session that runs the model through the TensorRT provider, in half
/// precision, for pictures of `size` (width, height) only: made for one
/// size TensorRT's form of the model is made when the session is opened
/// and is as fast as it gets. What TensorRT would not take of the model
/// (nothing, so far) is the CUDA provider's.
fn open_fast_session(model: &Path, input: &str, size: (u32, u32)) -> Result<Session, String> {
    preload_tensorrt()?;
    let folder = engines_folder(&paths::data_dir(), size);
    let timings = paths::data_dir().join(ENGINES_FOLDER);
    let shape = format!("{input}:1x3x{}x{}", size.1, size.0);
    open_kept_or_anew(&folder, || {
        let tensorrt = ort::ep::TensorRT::default()
            .with_fp16(true)
            .with_engine_cache(true)
            .with_engine_cache_path(folder.display())
            .with_timing_cache(true)
            // What TensorRT learned of this GPU while it made one size
            // shortens the making of the next: kept for all of them.
            .with_timing_cache_path(timings.display())
            .with_profile_min_shapes(&shape)
            .with_profile_opt_shapes(&shape)
            .with_profile_max_shapes(&shape)
            .build()
            .error_on_failure();
        open_session_with(model, &[tensorrt, ort::ep::CUDA::default().build()])
    })
}

/// A session for the model, on the GPU through the CUDA provider if `gpu`,
/// where every failure of the provider is an error.
fn open_session(model: &Path, gpu: bool) -> Result<Session, String> {
    if gpu {
        open_session_with(
            model,
            &[ort::ep::CUDA::default().build().error_on_failure()],
        )
    } else {
        open_session_with(model, &[])
    }
}

/// A session for the model with `providers`, the first before the next.
fn open_session_with(
    model: &Path,
    providers: &[ort::ep::ExecutionProviderDispatch],
) -> Result<Session, String> {
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
    if !providers.is_empty() {
        builder = builder
            .with_execution_providers(providers)
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
            fast: Fast::Unasked,
            tensorrt: may_use_tensorrt(std::env::var_os(TENSORRT_ENV), has_tensorrt_provider),
            told: runs_on.clone(),
            input,
            model: model.to_owned(),
            runs_on,
            last_pixels: 0,
            gives_back: true,
        })
    }

    /// Estimate the nearness of every pixel of `image`, which the model is
    /// shown with its long side at about `long_side` pixels (one of
    /// [`DETAIL`]). The map comes back at that working size, in the
    /// model's own numbers ([`RawDepth`]); the renderer stretches it to the
    /// image.
    pub fn estimate_raw(
        &mut self,
        image: &RgbaImage,
        long_side: u32,
        purpose: Purpose,
    ) -> Result<RawDepth, String> {
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 {
            return Err("The source contains no image.".into());
        }
        let (w, h) = model_size(width, height, long_side);
        // A live screen's map is TensorRT's where it can be ([`Fast`]):
        // only where the CUDA provider works, which TensorRT needs too.
        if purpose == Purpose::Live && self.tensorrt && self.runs_on == RunsOn::Gpu {
            let (model, input) = (self.model.clone(), self.input.clone());
            let fast = self.fast.session_for(
                (w, h),
                one_at_a_time(move || open_fast_session(&model, &input, (w, h))),
            );
            if let Some(session) = fast {
                match run(session, &self.input, image, (w, h), None) {
                    Ok(map) => {
                        // The CUDA provider's session made the maps until
                        // this one and keeps the room of its last (below),
                        // which nothing uses from here on: measured on an
                        // RTX 5090 with a 3840 x 2160 screen, 5.5 GB of
                        // the GPU's memory were held at the finest detail
                        // with TensorRT at work, more than without it
                        // (5.0 GB), and 3.2 GB at the next finest. One
                        // more run of that session, asked to give the
                        // room back, lets go of it; once, for a picture of
                        // the size it has the room for.
                        if self.gives_back && self.last_pixels == u64::from(w) * u64::from(h) {
                            let _ =
                                run(&mut self.session, &self.input, image, (w, h), Some("gpu:0"));
                        }
                        self.last_pixels = 0;
                        self.tell();
                        return Ok(map);
                    }
                    // The map is made as it was before there was TensorRT.
                    Err(why) => self.fast = Fast::Refused(why),
                }
            }
        }
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
        let not_taken = std::cell::Cell::new(false);
        let map = run_or_fall_back(
            &mut self.session,
            &mut self.runs_on,
            |session, runs_on| {
                // The room is the GPU's where the model runs there.
                let room = give_back.then_some(match runs_on {
                    RunsOn::Processor { .. } => "cpu:0",
                    _ => "gpu:0",
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
        self.tell();
        map
    }

    /// Tell where the model runs when that is no longer what was told.
    fn tell(&mut self) {
        let now = shown(&self.runs_on, &self.fast);
        if now != self.told {
            note_runs_on(&now);
            self.told = now;
        }
    }
}

/// What is told of a model that runs at `runs_on` with TensorRT's form of
/// it as far as `fast`.
fn shown<S>(runs_on: &RunsOn, fast: &Fast<S>) -> RunsOn {
    match (runs_on, fast) {
        (RunsOn::Gpu, Fast::Making { .. }) => RunsOn::Preparing,
        (RunsOn::Gpu, Fast::Ready { .. }) => RunsOn::TensorRt,
        (RunsOn::Gpu, Fast::Refused(why)) => RunsOn::GpuOnly {
            tensorrt_refused: why.clone(),
        },
        _ => runs_on.clone(),
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
) -> Result<RawDepth, String> {
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
    let dims: Vec<i64> = shape.iter().copied().collect();
    answer(&dims, values)
}

/// The model's output, `values` in the shape `dims`, as a map.
fn answer(dims: &[i64], values: &[f32]) -> Result<RawDepth, String> {
    // [1, H, W] or [1, 1, H, W]: the last two dimensions are the map.
    let (out_h, out_w) = match dims {
        [.., h, w] if *h > 0 && *w > 0 => (*h as u32, *w as u32),
        _ => return Err(format!("The depth model returned shape {dims:?}.")),
    };
    // Depth Anything's output grows with nearness, which is what the
    // renderer's map means too. The numbers stay as they are: whoever
    // asked brings them to 0..=1, a still image at once and a live screen
    // against the maps before ([`RawDepth`]).
    RawDepth::new(out_w, out_h, values.to_vec())
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

    /// Ask `fast` for `size` until it answers otherwise than "not yet".
    fn awaited(fast: &mut Fast<&'static str>, size: (u32, u32)) -> Option<&'static str> {
        for _ in 0..2000 {
            let never = || -> Result<&'static str, String> { panic!("made a second time") };
            if let Some(session) = fast.session_for(size, never) {
                return Some(*session);
            }
            if matches!(fast, Fast::Refused(_)) {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("nothing was made");
    }

    #[test]
    fn tensorrts_form_of_the_model_is_made_aside_and_takes_over_when_it_is_there() {
        use std::sync::mpsc::channel;
        let mut fast = Fast::Unasked;
        assert_eq!(shown(&RunsOn::Gpu, &fast), RunsOn::Gpu);
        // Asked for: nothing yet, and the maps go on as before while it
        // is made, for as long as that takes.
        let (finish, finished) = channel::<()>();
        let made = fast.session_for((1274, 714), move || {
            finished.recv().unwrap();
            Ok("for 1274 x 714")
        });
        assert!(made.is_none());
        assert_eq!(shown(&RunsOn::Gpu, &fast), RunsOn::Preparing);
        for _ in 0..3 {
            let again = fast.session_for((1274, 714), || panic!("made a second time"));
            assert!(again.is_none());
        }
        // Made: it is the one for that size from then on.
        finish.send(()).unwrap();
        assert_eq!(awaited(&mut fast, (1274, 714)), Some("for 1274 x 714"));
        assert_eq!(awaited(&mut fast, (1274, 714)), Some("for 1274 x 714"));
        assert_eq!(shown(&RunsOn::Gpu, &fast), RunsOn::TensorRt);
        // Another depth detail: that size's is made, and nothing made for
        // another size is run meanwhile.
        let other = fast.session_for((518, 294), || Ok("for 518 x 294"));
        assert!(other.is_none());
        assert_eq!(awaited(&mut fast, (518, 294)), Some("for 518 x 294"));
        // A processor's model has nothing to do with any of it.
        let on_processor = RunsOn::Processor { gpu_refused: None };
        assert_eq!(shown(&on_processor, &fast), on_processor);
    }

    #[test]
    fn changing_detail_waits_for_the_current_engine_build() {
        let mut fast = Fast::Unasked;
        let (finish, wait) = std::sync::mpsc::channel::<()>();
        fast.session_for((518, 294), move || {
            wait.recv().unwrap();
            Ok("first")
        });
        for size in [(770, 434), (518, 294), (1274, 714)] {
            assert!(fast.session_for(size, || Ok("overlap")).is_none());
            assert!(matches!(
                fast,
                Fast::Making {
                    size: (518, 294),
                    ..
                }
            ));
        }
        finish.send(()).unwrap();
        for _ in 0..2000 {
            if let Some(session) = fast.session_for((1274, 714), || Ok("latest")) {
                assert_eq!(*session, "latest");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("the requested build did not finish");
    }

    #[test]
    fn a_refusal_that_comes_after_the_detail_changed_is_kept() {
        // TensorRT cannot be used at all, and says so only after another
        // detail was chosen. With whatever was made for another size let
        // go of, its refusal went too: it was asked again at every change
        // of the detail, and neither the page nor the log said why the
        // GPU went on without it.
        let mut fast: Fast<&str> = Fast::Unasked;
        let (finish, wait) = std::sync::mpsc::channel::<()>();
        fast.session_for((518, 294), move || {
            wait.recv().unwrap();
            Err("libnvinfer.so.10: cannot open".to_owned())
        });
        assert!(fast.session_for((770, 434), || panic!("overlap")).is_none());
        finish.send(()).unwrap();
        for _ in 0..2000 {
            if matches!(fast, Fast::Refused(_)) {
                break;
            }
            assert!(fast
                .session_for((770, 434), || panic!("asked again"))
                .is_none());
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            shown(&RunsOn::Gpu, &fast),
            RunsOn::GpuOnly {
                tensorrt_refused: "libnvinfer.so.10: cannot open".into()
            }
        );
        assert!(fast
            .session_for((1274, 714), || panic!("asked again"))
            .is_none());
    }

    #[test]
    fn what_a_stopped_live_screen_left_being_made_is_finished_before_another_is_begun() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        use std::sync::mpsc::{channel, Receiver};
        use std::sync::Arc;
        // How many makers are at work, and the most there were at once.
        let at_work = Arc::new((AtomicUsize::new(0), AtomicUsize::new(0)));
        let maker = |name: &'static str, wait: Receiver<()>| {
            let at_work = at_work.clone();
            one_at_a_time(move || {
                let now = at_work.0.fetch_add(1, SeqCst) + 1;
                at_work.1.fetch_max(now, SeqCst);
                wait.recv().unwrap();
                at_work.0.fetch_sub(1, SeqCst);
                Ok(name)
            })
        };
        let wait_for = |what: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while !what() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        assert_eq!(MAKING.load(SeqCst), 0);
        // A live screen's model has TensorRT's form of it made. The live
        // screen is stopped and the model let go of: the making goes on.
        let (finish_first, wait) = channel();
        let mut first = Fast::Unasked;
        assert!(first
            .session_for((1274, 714), maker("first", wait))
            .is_none());
        wait_for(&|| at_work.0.load(SeqCst) == 1);
        drop(first);
        // Started again: another model, which asks for the same at once
        // (and would have it at once, were it made).
        let (finish_second, wait) = channel();
        finish_second.send(()).unwrap();
        let mut second = Fast::Unasked;
        assert!(second
            .session_for((1274, 714), maker("second", wait))
            .is_none());
        // Both are counted, the one that waits too: the program does not
        // leave as programs do while either is there.
        wait_for(&|| MAKING.load(SeqCst) == 2);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(at_work.0.load(SeqCst), 1);
        assert!(second
            .session_for((1274, 714), || panic!("asked again"))
            .is_none());
        finish_first.send(()).unwrap();
        assert_eq!(awaited(&mut second, (1274, 714)), Some("second"));
        assert_eq!(at_work.1.load(SeqCst), 1, "two were made at once");
        wait_for(&|| MAKING.load(SeqCst) == 0);
        // Nothing being made: the program is not ended here.
        leave_at_once_while_making(3);
    }

    #[test]
    fn a_tensorrt_that_refuses_is_told_of_and_not_asked_again() {
        let mut fast = Fast::Unasked;
        let made = fast.session_for((518, 294), || {
            Err("libnvinfer.so.10: cannot open".to_owned())
        });
        assert!(made.is_none());
        assert_eq!(awaited(&mut fast, (518, 294)), None);
        assert_eq!(
            shown(&RunsOn::Gpu, &fast),
            RunsOn::GpuOnly {
                tensorrt_refused: "libnvinfer.so.10: cannot open".into()
            }
        );
        // Not for this size and not for another.
        for size in [(518, 294), (770, 434)] {
            assert!(fast
                .session_for(size, || -> Result<&str, String> { panic!("asked again") })
                .is_none());
        }
        let said = shown(&RunsOn::Gpu, &fast);
        assert!(said.describe().contains("libnvinfer.so.10: cannot open"));
        assert!(said.for_log().contains("TensorRT provider said"));
        // What it is made by ends without a word: that is a refusal too.
        let mut fast: Fast<&str> = Fast::Unasked;
        assert!(fast
            .session_for((518, 294), || panic!("the maker broke"))
            .is_none());
        assert_eq!(awaited(&mut fast, (518, 294)), None);
    }

    #[test]
    fn a_kept_form_of_the_model_that_cannot_be_opened_is_made_anew_once() {
        use std::cell::Cell;
        let data = tempfile::tempdir().unwrap();
        let folder = engines_folder(data.path(), (1274, 714));
        assert!(folder.ends_with("depth-engines/1274x714"));
        let opened = Cell::new(0);
        let open = |works_from: u32| {
            let (opened, folder) = (&opened, &folder);
            move || {
                opened.set(opened.get() + 1);
                assert!(
                    folder.is_dir(),
                    "the folder is there before TensorRT writes"
                );
                if opened.get() >= works_from {
                    std::fs::write(folder.join("model_fp16_sm120.engine"), b"made").unwrap();
                    Ok(opened.get())
                } else {
                    Err("the engine was made by another TensorRT".to_owned())
                }
            }
        };
        // Nothing kept: what TensorRT says of making it is the answer.
        assert_eq!(
            open_kept_or_anew(&folder, open(9)),
            Err("the engine was made by another TensorRT".to_owned())
        );
        assert_eq!(opened.take(), 1);
        assert_eq!(open_kept_or_anew(&folder, open(1)), Ok(1));
        assert!(holds_an_engine(&folder));
        // Kept and opened: once.
        opened.set(0);
        assert_eq!(open_kept_or_anew(&folder, open(1)), Ok(1));
        // Kept by another TensorRT: cleared away, and made anew.
        std::fs::write(folder.join("old.profile"), b"old").unwrap();
        opened.set(0);
        assert_eq!(open_kept_or_anew(&folder, open(2)), Ok(2));
        assert!(!folder.join("old.profile").exists());
        assert!(holds_an_engine(&folder));
        let aside = data.path().join("depth-engines/1274x714.kept");
        assert!(!aside.exists());
        // And when that fails too, it is not tried a third time. Then it
        // was not the kept one that TensorRT could not do with (its own
        // library is missing, the GPU's memory is taken): the kept one is
        // there for the next start, as it was.
        std::fs::write(folder.join("model_fp16_sm120.engine"), b"kept").unwrap();
        std::fs::write(folder.join("model_fp16_sm120.profile"), b"its profile").unwrap();
        opened.set(0);
        assert!(open_kept_or_anew(&folder, open(9)).is_err());
        assert_eq!(opened.get(), 2);
        assert_eq!(
            std::fs::read(folder.join("model_fp16_sm120.engine")).unwrap(),
            b"kept"
        );
        assert!(folder.join("model_fp16_sm120.profile").exists());
        assert!(!aside.exists());
        // What an earlier run left aside (it was ended there) is no
        // obstacle to the next one.
        std::fs::create_dir(&aside).unwrap();
        std::fs::write(aside.join("left.engine"), b"left").unwrap();
        opened.set(0);
        assert_eq!(open_kept_or_anew(&folder, open(2)), Ok(2));
        assert!(!aside.exists());
        assert_eq!(
            std::fs::read(folder.join("model_fp16_sm120.engine")).unwrap(),
            b"made"
        );
    }

    #[test]
    fn an_unusable_override_allows_the_installed_tensorrt_folder_to_be_tried() {
        let data = tempfile::tempdir().unwrap();
        let folders = [data.path().join("override"), data.path().join("installed")];
        for folder in &folders {
            std::fs::create_dir(folder).unwrap();
            for name in ["libnvinfer.so.10", "libnvonnxparser.so.10"] {
                std::fs::write(folder.join(name), []).unwrap();
            }
        }
        for working in &folders {
            let mut tried = Vec::new();
            let preloaded = preload_tensorrt_from(&folders, |path| {
                tried.push(path.to_owned());
                path.starts_with(working)
            });
            assert_eq!(preloaded, Ok(()));
            let mut expected = tensorrt_libraries(&folders[0]);
            if working == &folders[1] {
                expected.extend(tensorrt_libraries(working));
            }
            assert_eq!(tried, expected);
        }
    }

    #[cfg(all(target_pointer_width = "64", target_endian = "little"))]
    #[test]
    fn tensorrt_is_left_alone_where_one_of_its_libraries_is_cut_short() {
        let data = tempfile::tempdir().unwrap();
        let folders = [data.path().join("named"), data.path().join("installed")];
        let whole = library(true);
        // As the set-up leaves the folder: the two the provider is linked
        // against, and what TensorRT opens by itself.
        let names = [
            "libnvinfer.so.10",
            "libnvonnxparser.so.10",
            "libnvinfer_plugin.so.10",
            "libnvinfer_builder_resource_sm120.so.10.16.1",
        ];
        for folder in &folders {
            std::fs::create_dir(folder).unwrap();
            for name in names {
                std::fs::write(folder.join(name), &whole).unwrap();
            }
            // Not TensorRT's, and looked at where it is loaded.
            std::fs::write(folder.join("libonnxruntime.so.1.32.0"), &whole[..3000]).unwrap();
        }
        assert_eq!(cut_tensorrt_library(&folders), None);
        let mut opened = 0;
        assert_eq!(
            preload_tensorrt_from(&folders, |_| {
                opened += 1;
                true
            }),
            Ok(())
        );
        assert_eq!(opened, 2);
        // Each of them cut short, in the folder that is named and in the
        // installed one: nothing of TensorRT is opened, not the whole ones
        // of the other folder either, and the file is named at the end.
        for folder in &folders {
            for name in names {
                let library = folder.join(name);
                std::fs::write(&library, &whole[..3000]).unwrap();
                let refusal = preload_tensorrt_from(&folders, |path| {
                    panic!(
                        "opened {} beside a library that is cut short",
                        path.display()
                    )
                })
                .unwrap_err();
                assert!(
                    refusal.contains("not whole (3000 of 4368 bytes)")
                        && refusal.ends_with(&library.display().to_string()),
                    "{refusal}"
                );
                // The page has the end of it: what is wrong with which
                // file, however long the folder's name is.
                let on_the_page = RunsOn::GpuOnly {
                    tensorrt_refused: refusal,
                }
                .describe();
                assert!(
                    on_the_page.contains(&format!("It is cut short: {}", library.display())),
                    "{on_the_page}"
                );
                std::fs::write(&library, &whole).unwrap();
            }
        }
        // A folder that is not there holds nothing that is cut short.
        assert_eq!(
            cut_tensorrt_library(&[data.path().join("missing"), folders[1].clone()]),
            None
        );
    }

    #[test]
    fn a_runtime_of_the_users_own_is_used_before_the_systems_and_after_a_named_one() {
        let data = tempfile::tempdir().unwrap();
        let system = PathBuf::from(RUNTIME_LIBRARY);
        assert_eq!(runtime_library(None, data.path()), system);
        assert_eq!(runtime_library(Some("".into()), data.path()), system);
        let folder = data.path().join(RUNTIME_FOLDER);
        std::fs::create_dir(&folder).unwrap();
        // TensorRT's libraries alone are no runtime.
        for name in [
            "libnvonnxparser.so.10",
            "libnvinfer.so.10",
            "libnvinfer.so.10.16.1",
            "libnvinfer_plugin.so.10",
            "libnvinfer_builder_resource_sm120.so.10.16.1",
        ] {
            std::fs::write(folder.join(name), b"").unwrap();
        }
        assert_eq!(runtime_library(None, data.path()), system);
        std::fs::write(folder.join(RUNTIME_IN_FOLDER), b"").unwrap();
        assert_eq!(
            runtime_library(None, data.path()),
            folder.join(RUNTIME_IN_FOLDER)
        );
        assert_eq!(
            runtime_library(Some("/opt/ort/libonnxruntime.so".into()), data.path()),
            PathBuf::from("/opt/ort/libonnxruntime.so")
        );
        // The two the provider is linked against, the library first.
        assert_eq!(
            tensorrt_libraries(&folder),
            [
                folder.join("libnvinfer.so.10"),
                folder.join("libnvonnxparser.so.10")
            ]
        );
        assert!(tensorrt_libraries(&data.path().join("none")).is_empty());
    }

    #[test]
    fn tensorrt_is_asked_for_where_the_runtime_has_it_unless_the_user_says_no() {
        assert!(may_use_tensorrt(None, || true));
        assert!(may_use_tensorrt(Some("1".into()), || true));
        assert!(!may_use_tensorrt(None, || false));
        assert!(!may_use_tensorrt(Some("0".into()), || panic!("asked")));
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
    fn the_models_answer_is_handed_on_with_its_own_numbers() {
        // What the model said of four pixels, in either shape it uses.
        let said = [3.5f32, 7.0, 5.25, 40.0];
        for dims in [&[1i64, 2, 2][..], &[1, 1, 2, 2]] {
            let raw = answer(dims, &said).unwrap();
            assert_eq!((raw.width(), raw.height()), (2, 2));
            // Not brought to 0..=1: a live screen's maps are steadied
            // against each other with the ranges they came with.
            assert_eq!(raw.values(), said);
            // A still image's map is, as it always was.
            assert_eq!(
                raw.normalized().unwrap(),
                DepthMap::normalized(2, 2, said.to_vec()).unwrap()
            );
        }
        let wide = answer(&[1, 1, 4], &said).unwrap();
        assert_eq!((wide.width(), wide.height()), (4, 1));
        // Answers that are no map are told as before.
        assert_eq!(
            answer(&[1, 0, 4], &[]).unwrap_err(),
            "The depth model returned shape [1, 0, 4]."
        );
        assert_eq!(
            answer(&[4], &said).unwrap_err(),
            "The depth model returned shape [4]."
        );
        assert_eq!(
            answer(&[1, 2, 2], &said[..3]).unwrap_err(),
            DibrError::BadDepthMap.to_string()
        );
        assert_eq!(
            answer(&[1, 2, 2], &[1.0, f32::NAN, 2.0, 3.0]).unwrap_err(),
            DibrError::NotFinite.to_string()
        );
        assert_eq!(
            RawDepth::new(0, 2, Vec::new()).unwrap_err(),
            DibrError::Empty.to_string()
        );
        // Numbers further apart than a number can say would make a map of
        // "not a number" whoever brought them to 0..=1.
        assert!(answer(&[1, 1, 2], &[-f32::MAX, f32::MAX]).is_err());
    }

    #[test]
    fn the_edges_of_a_map_are_put_where_the_rest_is_farthest() {
        // Four columns, five rows; the outermost rows and the left column
        // hold what the model said of black bars: nearer than the rest.
        let said: Vec<f32> = (0..20)
            .map(|i| match (i % 4, i / 4) {
                (_, 0) | (_, 4) | (0, _) => 9.0,
                (x, y) => 2.0 + (x + y) as f32,
            })
            .collect();
        let mut map = RawDepth::new(4, 5, said.clone()).unwrap();
        map.far_at_the_edges(1, 1, 1, 0);
        let expected: Vec<f32> = (0..20)
            .map(|i| match (i % 4, i / 4) {
                (_, 0) | (_, 4) | (0, _) => 4.0,
                (x, y) => 2.0 + (x + y) as f32,
            })
            .collect();
        assert_eq!(map.values(), expected);
        // The range is the rest's: 4 (far) to 8.
        let shown = map.clone().normalized().unwrap();
        assert_eq!(
            (shown.values()[0], shown.values()[5], shown.values()[15]),
            (0.0, 0.0, 1.0)
        );
        // Nothing named, or nothing left: the map as it was.
        for edges in [
            (0, 0, 0, 0),
            (3, 2, 0, 0),
            (5, 0, 0, 0),
            (0, 0, 2, 2),
            (0, 0, 0, 4),
        ] {
            let mut map = RawDepth::new(4, 5, said.clone()).unwrap();
            map.far_at_the_edges(edges.0, edges.1, edges.2, edges.3);
            assert_eq!(map.values(), said, "{edges:?}");
        }
    }

    #[test]
    fn a_model_that_is_wanted_again_is_not_let_go_of_by_whoever_wanted_it_before() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        // Nobody wants it: it goes.
        let model = Mutex::new(Some(1u32));
        assert!(unload_from(&model, || true));
        assert_eq!(*model.lock().unwrap(), None);
        // (and only then is there anything to give back to the system)
        assert!(!unload_from(&model, || true));
        assert_eq!(*model.lock().unwrap(), None);
        let wanted = Mutex::new(Some(1u32));
        assert!(!unload_from(&wanted, || false));
        assert_eq!(*wanted.lock().unwrap(), Some(1));

        // A live screen has ended and counted itself out (0 are left); its
        // letting go waits for the map being made. Meanwhile another has
        // begun, counted itself in, and is given the model next.
        let model = Arc::new(Mutex::new(Some(1u32)));
        let users = Arc::new(AtomicUsize::new(0));
        let in_use = model.lock().unwrap();
        let (ended_model, ended_users) = (model.clone(), users.clone());
        let ended = std::thread::spawn(move || {
            unload_from(&ended_model, || ended_users.load(Ordering::SeqCst) == 0)
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        users.fetch_add(1, Ordering::SeqCst);
        drop(in_use);
        ended.join().unwrap();
        // It was let go of all the same, having been found unused before
        // the wait, and loaded again for the next map. Asked at the time,
        // the count says it is wanted.
        assert_eq!(
            with_model(
                &model,
                || false,
                || panic!("the model in use was let go of"),
                |model| Ok(*model)
            )
            .unwrap(),
            Some(1)
        );
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
    fn a_frame_made_at_the_models_size_or_at_twice_it_is_shown_at_the_models_size() {
        // A live screen's frame comes at the size the model is to see it
        // at, or at twice that size (`live::model_input_size`). Told the
        // detail the frame was made for, the estimator comes to the same
        // size again; told the frame's long side, it took a frame of twice
        // the size for a request for twice the detail.
        let widths = (1..=16384)
            .step_by(89)
            .chain([1036, 1920, 2560, 3840, 5120, 7680, 16384]);
        for width in widths {
            for height in [1, 14, 200, 588, 1080, 1440, 1600, 2160, 4320, 16384] {
                for detail in DETAIL.map(|(side, _)| side) {
                    let model = model_size(width, height, detail);
                    let what = format!("{width} x {height} at {detail}");
                    assert_eq!(model_size(model.0, model.1, detail), model, "{what}");
                    assert_eq!(
                        model_size(model.0 * 2, model.1 * 2, detail),
                        model,
                        "{what}, at twice the size"
                    );
                }
            }
        }
        // (what 0.9.8 asked for: the frame's long side)
        assert_eq!(model_size(1036, 588, 1036), (1036, 588));
        assert_eq!(model_size(1540, 868, 1540), (1274, 714));
    }

    /// What the depth thread does to a live screen's frame before the model
    /// sees it, at each depth detail of a 3840 x 2160 screen. No model is
    /// needed. Run by hand:
    /// `cargo test --release -p any2odyssey depth::tests::input_timings -- --ignored --nocapture`
    #[test]
    #[ignore = "a measurement, not a check"]
    fn input_timings() {
        for (detail, name) in DETAIL {
            let (w, h) = model_size(3840, 2160, detail);
            let (fw, fh) = any2odyssey_core::live::model_input_size((3840, 2160), (w, h));
            let frame = RgbaImage::from_fn(fw, fh, |x, y| {
                let v = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).to_le_bytes();
                image::Rgba([v[0], v[1], v[2], 255])
            });
            let mut times: Vec<_> = (0..15)
                .map(|_| {
                    let start = std::time::Instant::now();
                    std::hint::black_box(planar_input(&frame, w, h));
                    start.elapsed()
                })
                .collect();
            times.sort();
            println!(
                "{name}: a frame of {fw}x{fh} to the model's {w}x{h}: {:?}",
                times[7]
            );
        }
    }

    #[test]
    fn a_frame_of_twice_the_models_size_is_halved_as_the_image_library_halved_it() {
        // Sizes that give one band and several, with rows left over.
        for (w, h) in [(1u32, 1u32), (2, 3), (37, 23), (258, 131)] {
            let frame = RgbaImage::from_fn(w * 2, h * 2, |x, y| {
                let v = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).to_le_bytes();
                image::Rgba([v[0], v[1] ^ v[3], v[2], v[3]])
            });
            let halved =
                image::imageops::resize(&frame, w, h, image::imageops::FilterType::Triangle);
            let ours = planar_input(&frame, w, h);
            let plane = (w * h) as usize;
            assert_eq!(ours.len(), plane * 3);
            let step = |channel: usize| 1.0 / 255.0 / STD[channel];
            for (index, pixel) in halved.pixels().enumerate() {
                for channel in 0..3 {
                    let theirs = (pixel.0[channel] as f32 / 255.0 - MEAN[channel]) / STD[channel];
                    let ours = ours[channel * plane + index];
                    // The same colour, or its neighbour where the sum lies
                    // on the border between two and the library's numbers
                    // with a fraction fall the other way.
                    assert!(
                        (ours - theirs).abs() <= step(channel) * 1.01,
                        "{w}x{h}, pixel {index}, colour {channel}: {ours} against {theirs}"
                    );
                }
            }
            // Not one in fifty is that neighbour.
            let other = halved
                .pixels()
                .enumerate()
                .flat_map(|(index, pixel)| (0..3).map(move |channel| (index, channel, pixel)))
                .filter(|(index, channel, pixel)| {
                    let theirs =
                        (pixel.0[*channel] as f32 / 255.0 - MEAN[*channel]) / STD[*channel];
                    (ours[channel * plane + index] - theirs).abs() > step(*channel) * 0.01
                })
                .count();
            assert!(other * 50 <= plane * 3, "{w}x{h}: {other} of {}", plane * 3);
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
