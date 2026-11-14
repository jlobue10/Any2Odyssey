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
    // for an abandoned one in the moment before its lock was taken.
    for _ in 0..4 {
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
            preload_cudnn();
            let builder = ort::init_from(&library).map_err(|e| {
                format!(
                    "ONNX Runtime could not be loaded ({}): {e}. Install your distribution's onnxruntime package (Arch: onnxruntime-cpu or onnxruntime-cuda).",
                    library.display()
                )
            })?;
            // An NVIDIA GPU when the installed runtime has the CUDA provider
            // and it starts; otherwise the runtime's CPU provider, silently.
            builder
                .with_name("any2odyssey")
                .with_execution_providers([ort::ep::CUDA::default().build().fail_silently()])
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

pub struct Estimator {
    session: Session,
    input: String,
}

/// Share one model between still images and live sessions. A stopped live
/// session must not start expensive work after waiting for another caller.
/// Cancellation does not interrupt an inference already inside the runtime.
pub fn estimate_unless_cancelled(
    image: &RgbaImage,
    long_side: u32,
    cancelled: impl Fn() -> bool,
) -> Result<Option<DepthMap>, String> {
    static ESTIMATOR: Mutex<Option<Estimator>> = Mutex::new(None);
    with_model(
        &ESTIMATOR,
        cancelled,
        || {
            let (model, origin) = locate_model()?;
            verify_installed(&model, origin, MODEL_SHA256)?;
            Estimator::load(&model)
        },
        |estimator| estimator.estimate(image, long_side),
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

impl Estimator {
    pub fn load(model: &Path) -> Result<Estimator, String> {
        let failed = |e: &dyn std::fmt::Display| {
            format!("Could not load the depth model {}: {e}", model.display())
        };
        // On the processor the model is given a few cores, not all of them:
        // it runs beside whatever shows its result (a live screen's
        // renderer, Leia's eye tracker under Wine), and a model that takes
        // every core makes all of those late. On a GPU the setting is moot.
        let mut builder = Session::builder(runtime()?)
            .map_err(|e| failed(&e))?
            .with_intra_threads(model_threads())
            .map_err(|e| failed(&e))?
            // Idle model threads sleep instead of spinning on their cores.
            .with_intra_op_spinning(false)
            .map_err(|e| failed(&e))?;
        let session = builder.commit_from_file(model).map_err(|e| failed(&e))?;
        let input = session
            .inputs()
            .first()
            .map(|input| input.name().to_owned())
            .ok_or("The depth model has no input.")?;
        if session.outputs().is_empty() {
            return Err("The depth model has no output.".into());
        }
        Ok(Estimator { session, input })
    }

    /// Estimate the nearness of every pixel of `image`, which the model is
    /// shown with its long side at about `long_side` pixels (one of
    /// [`DETAIL`]). The map comes back at that working size; the renderer
    /// stretches it to the image.
    pub fn estimate(&mut self, image: &RgbaImage, long_side: u32) -> Result<DepthMap, String> {
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 {
            return Err("The source contains no image.".into());
        }
        let (w, h) = model_size(width, height, long_side);
        let tensor = Tensor::from_array((
            [1usize, 3, h as usize, w as usize],
            planar_input(image, w, h),
        ))
        .map_err(|e| format!("Could not prepare the image for the depth model: {e}"))?;
        let outputs = self
            .session
            .run(ort::inputs![self.input.as_str() => tensor])
            .map_err(|e| format!("The depth model failed: {e}"))?;
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        remove_stale_downloads(dir.path(), day);
        assert!(!running.exists());
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
            assert!(!path.exists(), "{path:?} was left for another day");
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
        std::thread::scope(|scope| {
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
            std::fs::write(&go, b"").unwrap();
            assert_eq!(running.join().unwrap(), Ok(target.clone()));
        });
        assert_eq!(std::fs::read(&target).unwrap(), b"model");
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
        remove_stale_downloads(dir.path(), STALE_DOWNLOAD);
        assert!(!path.exists());
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
