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
const LONG_SIDE: u32 = 518;
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
    let folder = paths::data_dir().join("models");
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("Could not create {}: {e}", folder.display()))?;
    let target = folder.join(MODEL_FILE);
    let partial = folder.join(format!("{MODEL_FILE}.part"));
    let status = std::process::Command::new("curl")
        .args(["--fail", "--location", "--proto", "=https", "--output"])
        .arg(&partial)
        .arg(MODEL_URL)
        .status()
        .map_err(|e| format!("Could not run curl ({e}). Install curl, or download {MODEL_URL} and save it as {}.", target.display()))?;
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("The download failed (curl: {status})."));
    }
    let outcome = verify_download(&partial).and_then(|()| {
        std::fs::rename(&partial, &target)
            .map_err(|e| format!("Could not move the model into place: {e}"))
    });
    if outcome.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    outcome.map(|()| target)
}

fn verify_download(file: &Path) -> Result<(), String> {
    let actual = any2odyssey_core::engine::sha256_hex(file)
        .map_err(|e| format!("Could not read the downloaded file: {e}"))?;
    if actual.eq_ignore_ascii_case(MODEL_SHA256) {
        Ok(())
    } else {
        Err(format!(
            "The downloaded file is not the expected model (SHA-256 {actual}, expected {MODEL_SHA256}). Nothing was installed."
        ))
    }
}

/// The size the image is given to the model at: the long side near 518,
/// the aspect ratio kept, both sides a whole number of patches.
fn model_size(width: u32, height: u32) -> (u32, u32) {
    let long = width.max(height).max(1) as f32;
    let scale = LONG_SIDE as f32 / long;
    let side = |pixels: u32| {
        let patches = (pixels as f32 * scale / PATCH as f32).round().max(1.0) as u32;
        patches * PATCH
    };
    (side(width), side(height))
}

/// `image` as the model's input: planar RGB, normalized, at `width` x `height`.
fn planar_input(image: &RgbaImage, width: u32, height: u32) -> Vec<f32> {
    let resized =
        image::imageops::resize(image, width, height, image::imageops::FilterType::Triangle);
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

pub struct Estimator {
    session: Session,
    input: String,
}

impl Estimator {
    pub fn load(model: &Path) -> Result<Estimator, String> {
        let session = Session::builder(runtime()?)
            .and_then(|mut builder| builder.commit_from_file(model))
            .map_err(|e| format!("Could not load the depth model {}: {e}", model.display()))?;
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

    /// Estimate the nearness of every pixel of `image`. The map comes back
    /// at the model's working size; the renderer stretches it to the image.
    pub fn estimate(&mut self, image: &RgbaImage) -> Result<DepthMap, String> {
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 {
            return Err("The source contains no image.".into());
        }
        let (w, h) = model_size(width, height);
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
        assert_eq!(model_size(518, 518), (518, 518));
        assert_eq!(model_size(3840, 2160), (518, 294));
        assert_eq!(model_size(1080, 1920), (294, 518));
        for (w, h) in [(1, 1), (7, 5000), (1920, 1080), (640, 480), (13, 13)] {
            let (mw, mh) = model_size(w, h);
            assert!(mw >= PATCH && mh >= PATCH, "{w}x{h} -> {mw}x{mh}");
            assert_eq!((mw % PATCH, mh % PATCH), (0, 0), "{w}x{h} -> {mw}x{mh}");
            assert!(mw.max(mh) <= LONG_SIDE + PATCH, "{w}x{h} -> {mw}x{mh}");
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
        let error = verify_download(&file).unwrap_err();
        assert!(error.contains("not the expected model"), "{error}");
        assert!(verify_download(&dir.path().join("missing")).is_err());
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
