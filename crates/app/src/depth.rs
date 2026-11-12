//! Depth from a single image, for the stereo viewer's 2D → 3D mode.
//!
//! The model is Depth Anything V2 (small) in ONNX form, run by the system's
//! ONNX Runtime. Neither ships inside the program: the runtime library is
//! opened and the model file read when the first depth map is asked for, and
//! a missing one is an ordinary error message with what to install.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
/// Face, and the SHA-256 of that file. A different file is refused.
pub const MODEL_URL: &str =
    "https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/main/onnx/model.onnx";
pub const MODEL_SHA256: &str = "afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c";

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

/// Where a model file is looked for, in order: the override, the user's data
/// folder, then the system-wide folders a package installs into.
pub fn model_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os(MODEL_ENV).filter(|p| !p.is_empty()) {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(paths::data_dir().join("models").join(MODEL_FILE));
    for system in ["/usr/local/share/any2odyssey", "/usr/share/any2odyssey"] {
        candidates.push(Path::new(system).join("models").join(MODEL_FILE));
    }
    candidates
}

pub fn locate_model() -> Result<PathBuf, String> {
    let candidates = model_candidates();
    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| {
            format!(
                "The depth model is not installed. Put {MODEL_FILE} in {} (see Help: Windows & Linux).",
                paths::data_dir().join("models").display()
            )
        })
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
    // Exclusive creation keeps simultaneous installers independent and never
    // follows a pre-existing .part symlink. Curl writes through the already
    // open file, not a pathname it would reopen and truncate.
    let partial = tempfile::Builder::new()
        .prefix(".depth-download-")
        .tempfile_in(folder)
        .map_err(|e| format!("Could not create a model download: {e}"))?;
    let output = partial
        .reopen()
        .map_err(|e| format!("Could not open the model download: {e}"))?;
    let status = command
        .args(["--fail", "--location", "--proto", "=https", "--output", "-"])
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
        assert!(candidates.iter().all(|path| path
            .file_name()
            .is_some_and(|name| name == MODEL_FILE)
            || std::env::var_os(MODEL_ENV).is_some()));
        assert!(candidates
            .last()
            .unwrap()
            .starts_with("/usr/share/any2odyssey"));
    }
}
