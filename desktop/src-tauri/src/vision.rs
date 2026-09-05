//! Real neural face detection + embedding extraction (Task 5.1 of the
//! biometric roadmap), replacing the previous simulated/mocked probe vectors
//! that were fabricated client-side in JS (`generateNormalizedFaceEmbedding`).
//!
//! Pipeline: raw camera frame -> YuNet face detector (ONNX) -> 5-point
//! similarity-transform alignment -> 112x112 crop -> ArcFace recognizer
//! (ONNX, 512-d; Task 5.2) with SFace (128-d) fallback when the ArcFace
//! model file is absent — matching `desktop/src-tauri/src/face.rs`'s
//! existing `FaceVectorStore` contract exactly (dimension-agnostic cosine,
//! no changes needed there).
//!
//! The detector decode algorithm, NMS parameters, and recognizer alignment
//! math are transcribed from OpenCV's own C++ implementation
//! (`modules/objdetect/src/face_detect.cpp` and `face_recognize.cpp`) rather
//! than reverse-engineered, since a subtly wrong anchor-decode formula would
//! silently produce plausible-looking-but-garbage boxes.

use image::{imageops::FilterType, DynamicImage, Rgb, RgbImage};
use std::path::{Path, PathBuf};
use tract_onnx::prelude::*;

/// Locates the directory containing `face_detection_yunet_2023mar.onnx` and
/// `face_recognition_sface_2021dec.onnx`, trying (in order): the current
/// working directory layouts used by `cargo run`/`cargo tauri dev` from
/// either the repo root or `desktop/`, then paths relative to the running
/// executable (covers a packaged build where `tauri.conf.json`'s
/// `bundle.resources` places `models/` next to the binary).
pub fn find_models_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![
        PathBuf::from("desktop/models"),
        PathBuf::from("models"),
        PathBuf::from("../models"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join("models"));
            candidates.push(exe_dir.join("_up_").join("models"));
            candidates.push(exe_dir.join("resources").join("models"));
            candidates.push(exe_dir.join("resources").join("_up_").join("models"));
            candidates.push(exe_dir.join("..").join("models"));
        }
    }

    candidates.into_iter().find(|dir| {
        dir.join("face_detection_yunet_2023mar.onnx").is_file()
            && dir.join("face_recognition_sface_2021dec.onnx").is_file()
    })
}

const DET_INPUT_SIZE: u32 = 640;
const DET_STRIDES: [usize; 3] = [8, 16, 32];
const DET_SCORE_THRESHOLD: f32 = 0.6;
const DET_NMS_THRESHOLD: f32 = 0.3;
const REC_INPUT_SIZE: u32 = 112;

/// Canonical 5-point reference landmarks for a 112x112 aligned face
/// (right eye, left eye, nose tip, right mouth corner, left mouth corner).
/// Lifted verbatim from OpenCV's `FaceRecognizerSF::getSimilarityTransformMatrix`.
const REFERENCE_LANDMARKS_112: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

#[derive(Debug, Clone)]
pub struct DetectedFace {
    /// (x, y, w, h) bounding box in the ORIGINAL (un-letterboxed) image.
    pub rect: (f32, f32, f32, f32),
    /// 5 landmarks (right eye, left eye, nose, right mouth, left mouth) in
    /// original image coordinates.
    pub landmarks: [[f32; 2]; 5],
    pub score: f32,
}

type Plan = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

pub struct FaceEngine {
    detector: Plan,
    recognizer: Plan,
    /// 512-d ArcFace recognizer (Task 5.2). `None` when
    /// `face_recognition_arcface_w600k_mbf.onnx` is absent — the engine then
    /// serves legacy 128-d SFace embeddings via the same API.
    arcface: Option<Plan>,
}

impl FaceEngine {
    /// Loads both ONNX models from the given directory (Tauri's bundled
    /// `resources` dir in production, `desktop/models/` in dev).
    pub fn load(models_dir: &Path) -> Result<Self, String> {
        let detector_path = models_dir.join("face_detection_yunet_2023mar.onnx");
        let recognizer_path = models_dir.join("face_recognition_sface_2021dec.onnx");

        let detector = tract_onnx::onnx()
            .model_for_path(&detector_path)
            .map_err(|e| {
                format!(
                    "Failed to load YuNet detector at {:?}: {}",
                    detector_path, e
                )
            })?
            .into_optimized()
            .map_err(|e| format!("Failed to optimize YuNet detector graph: {}", e))?
            .into_runnable()
            .map_err(|e| format!("Failed to make YuNet detector runnable: {}", e))?;

        let recognizer = tract_onnx::onnx()
            .model_for_path(&recognizer_path)
            .map_err(|e| {
                format!(
                    "Failed to load SFace recognizer at {:?}: {}",
                    recognizer_path, e
                )
            })?
            .into_optimized()
            .map_err(|e| format!("Failed to optimize SFace recognizer graph: {}", e))?
            .into_runnable()
            .map_err(|e| format!("Failed to make SFace recognizer runnable: {}", e))?;

        let arcface = Self::load_arcface_opt(models_dir);

        Ok(Self {
            detector,
            recognizer,
            arcface,
        })
    }

    /// Detects the highest-confidence face in `image` and returns its
    /// bounding box + 5-point landmarks (in original image coordinates), or
    /// `None` if no face scores above [`DET_SCORE_THRESHOLD`].
    pub fn detect_largest_face(&self, image: &RgbImage) -> Result<Option<DetectedFace>, String> {
        let (orig_w, orig_h) = image.dimensions();
        if orig_w == 0 || orig_h == 0 {
            return Ok(None);
        }

        // The bundled YuNet export has a FIXED [1,3,640,640] input (no dynamic
        // shape support), so we resize directly to 640x640. This distorts
        // aspect ratio slightly for non-square webcam frames, which is fine
        // for detection robustness; we track separate x/y scale factors to
        // map boxes/landmarks back to original coordinates precisely.
        let resized =
            image::imageops::resize(image, DET_INPUT_SIZE, DET_INPUT_SIZE, FilterType::Triangle);
        let scale_x = orig_w as f32 / DET_INPUT_SIZE as f32;
        let scale_y = orig_h as f32 / DET_INPUT_SIZE as f32;

        // YuNet's blobFromImage call in OpenCV uses swapRB=false, and OpenCV
        // Mats are BGR by default, so the network expects BGR channel order.
        // Our source is RGB, so channels are swapped here.
        let input = rgb_to_nchw_tensor(&resized, true, 1.0)?;

        let outputs = self
            .detector
            .run(tvec!(input.into()))
            .map_err(|e| format!("YuNet inference failed: {}", e))?;

        // Graph output order (verified against the ONNX graph directly):
        // cls_8, cls_16, cls_32, obj_8, obj_16, obj_32,
        // bbox_8, bbox_16, bbox_32, kps_8, kps_16, kps_32
        let mut candidates = Vec::new();
        for (i, &stride) in DET_STRIDES.iter().enumerate() {
            let cols = (DET_INPUT_SIZE as usize) / stride;
            let rows = (DET_INPUT_SIZE as usize) / stride;

            let cls = outputs[i]
                .to_array_view::<f32>()
                .map_err(|e| e.to_string())?;
            let obj = outputs[i + 3]
                .to_array_view::<f32>()
                .map_err(|e| e.to_string())?;
            let bbox = outputs[i + 6]
                .to_array_view::<f32>()
                .map_err(|e| e.to_string())?;
            let kps = outputs[i + 9]
                .to_array_view::<f32>()
                .map_err(|e| e.to_string())?;

            let cls_v = cls.as_slice().ok_or("non-contiguous cls tensor")?;
            let obj_v = obj.as_slice().ok_or("non-contiguous obj tensor")?;
            let bbox_v = bbox.as_slice().ok_or("non-contiguous bbox tensor")?;
            let kps_v = kps.as_slice().ok_or("non-contiguous kps tensor")?;

            for r in 0..rows {
                for c in 0..cols {
                    let idx = r * cols + c;
                    let cls_score = cls_v[idx].clamp(0.0, 1.0);
                    let obj_score = obj_v[idx].clamp(0.0, 1.0);
                    let score = (cls_score * obj_score).sqrt();
                    if score < DET_SCORE_THRESHOLD {
                        continue;
                    }

                    let cx = (c as f32 + bbox_v[idx * 4]) * stride as f32;
                    let cy = (r as f32 + bbox_v[idx * 4 + 1]) * stride as f32;
                    let w = bbox_v[idx * 4 + 2].exp() * stride as f32;
                    let h = bbox_v[idx * 4 + 3].exp() * stride as f32;
                    let x1 = cx - w / 2.0;
                    let y1 = cy - h / 2.0;

                    let mut landmarks = [[0.0f32; 2]; 5];
                    for n in 0..5 {
                        landmarks[n][0] = (kps_v[idx * 10 + 2 * n] + c as f32) * stride as f32;
                        landmarks[n][1] = (kps_v[idx * 10 + 2 * n + 1] + r as f32) * stride as f32;
                    }

                    candidates.push(DetectedFace {
                        rect: (x1, y1, w, h),
                        landmarks,
                        score,
                    });
                }
            }
        }

        let kept = nms(candidates, DET_NMS_THRESHOLD);

        // Map the single best surviving detection back to original image
        // coordinates and return it (callers want "the" face, e.g. the
        // person standing at the turnstile).
        Ok(kept
            .into_iter()
            .max_by(|a, b| a.score.total_cmp(&b.score))
            .map(|mut f| {
                f.rect.0 *= scale_x;
                f.rect.1 *= scale_y;
                f.rect.2 *= scale_x;
                f.rect.3 *= scale_y;
                for p in f.landmarks.iter_mut() {
                    p[0] *= scale_x;
                    p[1] *= scale_y;
                }
                f
            }))
    }

    /// Aligns the face (5-point similarity transform to a canonical 112x112
    /// crop) and runs the SFace recognizer to produce a 128-d embedding.
    pub fn extract_embedding(
        &self,
        image: &RgbImage,
        face: &DetectedFace,
    ) -> Result<Vec<f32>, String> {
        let transform = similarity_transform(&face.landmarks);
        let aligned = warp_affine_112(image, &transform);

        // SFace's blobFromImage call uses swapRB=true (OpenCV Mats are BGR,
        // swapRB converts to RGB) so the network expects RGB order, matching
        // our source data directly (no channel swap needed).
        let input = rgb_to_nchw_tensor(&aligned, false, 1.0)?;

        let outputs = self
            .recognizer
            .run(tvec!(input.into()))
            .map_err(|e| format!("SFace inference failed: {}", e))?;

        let feature = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| e.to_string())?;
        Ok(feature.iter().copied().collect())
    }

    /// Convenience: detect + align + embed in one call. Returns `None` if no
    /// face was found above the confidence threshold.
    ///
    /// Prefers the 512-d ArcFace recognizer (Task 5.2) when its model loaded
    /// successfully, falling back to the legacy 128-d SFace embedding when
    /// the ArcFace model file is absent. Callers can distinguish via
    /// [`FaceEngine::embedding_dim`].
    pub fn detect_and_embed(
        &self,
        image: &RgbImage,
    ) -> Result<Option<(DetectedFace, Vec<f32>)>, String> {
        let Some(face) = self.detect_largest_face(image)? else {
            return Ok(None);
        };
        let embedding = if self.arcface.is_some() {
            self.extract_embedding_arcface(image, &face)?
        } else {
            self.extract_embedding(image, &face)?
        };
        Ok(Some((face, embedding)))
    }

    /// Dimensionality of embeddings produced by [`FaceEngine::detect_and_embed`]:
    /// 512 once the ArcFace model is present, otherwise legacy 128 (SFace).
    pub fn embedding_dim(&self) -> usize {
        if self.arcface.is_some() {
            ARCFACE_EMBEDDING_DIM
        } else {
            128
        }
    }

    /// Short recognizer tag for telemetry / API responses.
    pub fn recognizer_name(&self) -> &'static str {
        if self.arcface.is_some() {
            "arcface-w600k-mbf"
        } else {
            "sface-2021dec"
        }
    }
}

// --- Task 5.2: 512-d ArcFace embeddings (InsightFace `w600k_mbf`) ---
//
// The bundled SFace recognizer only outputs 128-d (`fc1: [1,128]`). This
// section upgrades the pipeline to 512-d ArcFace/CosFace representations
// using InsightFace's `buffalo_s` pack model `w600k_mbf.onnx` (MobileFaceNet
// trained with ArcFace loss on WebFace600K) — chosen over `buffalo_l`'s
// ResNet-50 (`w600k_r50.onnx`, ~170MB) because it is ~10x smaller on disk
// while keeping the same 512-d output contract, input geometry (112x112),
// and preprocessing spec.
//
// Reference spec (InsightFace `model_zoo/arcface_onnx.py`, i.e.
// `cv2.dnn.blobFromImage(img, 1.0/127.5, (112,112), (127.5,127.5,127.5),
// swapRB=True)`): RGB channel order, per-pixel `(x - 127.5) / 127.5`
// normalization, input `input.1: [N,3,112,112]`, output `516: [1,512]`
// (opset 11, MobileFaceNet backbone). Verified against the real
// `w600k_mbf.onnx` with `python -c "import onnx; ..."` before writing the
// decode below — same rigor as the YuNet/SFace/YOLOv8 integrations above.
//
// Migration note: pre-existing 128-d member vectors stay in SQLite and keep
// matching 128-d probes (length mismatch yields cosine 0.0, never a false
// accept), but members must be re-enrolled once to obtain 512-d vectors —
// `upsert_with_expiry` replaces a member's vectors wholesale, so a single
// re-enrollment fully converts that member.

/// InsightFace ArcFace recognizer filename inside the models directory.
pub const ARCFACE_MODEL_FILENAME: &str = "face_recognition_arcface_w600k_mbf.onnx";
/// Dimensionality of ArcFace embeddings (InsightFace `fc1` head).
pub const ARCFACE_EMBEDDING_DIM: usize = 512;

impl FaceEngine {
    /// Attempts to load the ArcFace recognizer, returning `None` (with a
    /// warning) when the model file is absent so the SFace fallback path
    /// keeps working. Called from [`FaceEngine::load`].
    fn load_arcface_opt(models_dir: &Path) -> Option<Plan> {
        let path = models_dir.join(ARCFACE_MODEL_FILENAME);
        if !path.is_file() {
            tracing::warn!(
                "ArcFace model not found at {:?} — falling back to 128-d SFace embeddings",
                path
            );
            return None;
        }
        // NOTE (verified 2026-09-03): this InsightFace export carries a
        // degenerate batch dimension on its input (`dim_value=0`, no
        // `dim_param`) — an export artifact, since the model genuinely
        // expects `[1,3,112,112]`. tract's shape analysis chokes on the
        // zero batch (`Failed analyse for node ... ConvHir`, reproduced on
        // both tract 0.21 and 0.23), so the fact is overridden here with the
        // documented concrete shape before optimizing. No model bytes are
        // modified; verified end-to-end (load + optimize + infer [1,512]).
        let load = (|| -> Result<Plan, String> {
            let mut model = tract_onnx::onnx()
                .model_for_path(&path)
                .map_err(|e| format!("Failed to load ArcFace model at {:?}: {}", path, e))?;
            model
                .set_input_fact(0, f32::fact([1usize, 3, 112, 112]).into())
                .map_err(|e| format!("Failed to set ArcFace input fact: {:?}", e))?;
            let plan = model
                .into_optimized()
                .map_err(|e| format!("Failed to optimize ArcFace graph: {}", e))?
                .into_runnable()
                .map_err(|e| format!("Failed to make ArcFace runnable: {}", e))?;
            Ok(plan)
        })();
        match load {
            Ok(plan) => {
                tracing::info!("Loaded 512-d ArcFace recognizer from {:?}", path);
                Some(plan)
            }
            Err(e) => {
                tracing::error!("{} — falling back to SFace", e);
                // Also stderr: unit tests never init a tracing subscriber, so
                // without this the concrete failure would be invisible there.
                eprintln!("ArcFace load failed (SFace fallback): {}", e);
                None
            }
        }
    }

    /// Aligns the face (same 5-point similarity transform + 112x112 crop as
    /// [`FaceEngine::extract_embedding`]) and runs the ArcFace recognizer to
    /// produce a 512-d embedding. Returns an error when the ArcFace model did
    /// not load — callers should use [`FaceEngine::detect_and_embed`], which
    /// handles the SFace fallback automatically.
    pub fn extract_embedding_arcface(
        &self,
        image: &RgbImage,
        face: &DetectedFace,
    ) -> Result<Vec<f32>, String> {
        let arcface = self
            .arcface
            .as_ref()
            .ok_or_else(|| "ArcFace recognizer unavailable: model file missing".to_string())?;
        let transform = similarity_transform(&face.landmarks);
        let aligned = warp_affine_112(image, &transform);

        // InsightFace normalization: (RGB - 127.5) / 127.5 (see module docs).
        let input = rgb_to_nchw_tensor_normalized(&aligned, false, 127.5, 127.5)?;

        let outputs = arcface
            .run(tvec!(input.into()))
            .map_err(|e| format!("ArcFace inference failed: {}", e))?;

        let feature = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| e.to_string())?;
        let embedding: Vec<f32> = feature.iter().copied().collect();
        if embedding.len() != ARCFACE_EMBEDDING_DIM {
            return Err(format!(
                "Unexpected ArcFace output dim: got {}, want {}",
                embedding.len(),
                ARCFACE_EMBEDDING_DIM
            ));
        }
        Ok(embedding)
    }
}

/// Converts an RGB image into an NCHW `[1,3,H,W]` f32 tensor with
/// `(x - mean) / std` per-pixel normalization, e.g. InsightFace ArcFace's
/// `(x - 127.5) / 127.5`. `swap_to_bgr` mirrors `swapRB` semantics for
/// BGR-trained models (pass `false` for InsightFace, which wants RGB).
fn rgb_to_nchw_tensor_normalized(
    img: &RgbImage,
    swap_to_bgr: bool,
    mean: f32,
    std: f32,
) -> Result<Tensor, String> {
    let (w, h) = img.dimensions();
    let mut data = vec![0f32; 3 * h as usize * w as usize];
    let plane = (h * w) as usize;
    for (x, y, px) in img.enumerate_pixels() {
        let Rgb([r, g, b]) = *px;
        let (c0, c1, c2) = if swap_to_bgr { (b, g, r) } else { (r, g, b) };
        let offset = (y as usize) * (w as usize) + x as usize;
        data[offset] = (c0 as f32 - mean) / std;
        data[plane + offset] = (c1 as f32 - mean) / std;
        data[2 * plane + offset] = (c2 as f32 - mean) / std;
    }
    Tensor::from_shape(&[1, 3, h as usize, w as usize], &data).map_err(|e| e.to_string())
}

// --- Task 5.4: Multi-Camera Anti-Tailgating Person Counter ---
//
// Runs the bundled `yolov8n.onnx` (standard Ultralytics export: input
// `images` [1,3,320,320], single output `output0` [1,84,2100] = 4 box coords
// + 80 COCO class scores per anchor, channel-major layout) to count people
// inside a caller-supplied ROI on the overhead Camera 3 feed. Replaces the
// `Math.sin(...)`-based fake "transit density" heuristic that previously
// stood in for this in `desktop/webview/static/js/app.js`.

const YOLO_INPUT_SIZE: u32 = 320;
const YOLO_NUM_ANCHORS: usize = 2100;
const YOLO_NUM_CLASSES: usize = 80;
const YOLO_PERSON_CLASS_INDEX: usize = 0; // COCO class 0 = "person"
const YOLO_CONF_THRESHOLD: f32 = 0.45;
const YOLO_NMS_THRESHOLD: f32 = 0.45;

/// MOG-lite motion background: per-pixel running Gaussian mean at 80x45.
/// No new dependencies (no opencv crate): the YOLO input is already resized,
/// so a second tiny downscale is nearly free. `sensitivity` (0-1, default
/// 0.5, tunable in Hardware Settings) scales the pixel-difference threshold.
const MOTION_W: u32 = 80;
const MOTION_H: u32 = 45;
const MOTION_ALPHA: f32 = 0.05;
const MOTION_BASE_THRESH: f32 = 25.0;

struct MotionState {
    mean: Vec<f32>,
    mask: Vec<bool>,
    initialized: bool,
    /// Fraction of ROI pixels moving on the last processed frame.
    last_roi_motion: f32,
}

pub struct PersonCounter {
    plan: Plan,
    motion: std::sync::Mutex<MotionState>,
    motion_sensitivity: std::sync::Mutex<f32>,
}

impl PersonCounter {
    pub fn load(models_dir: &Path) -> Result<Self, String> {
        let model_path = models_dir.join("yolov8n.onnx");
        let plan = tract_onnx::onnx()
            .model_for_path(&model_path)
            .map_err(|e| format!("Failed to load YOLOv8n model at {:?}: {}", model_path, e))?
            .into_optimized()
            .map_err(|e| format!("Failed to optimize YOLOv8n graph: {}", e))?
            .into_runnable()
            .map_err(|e| format!("Failed to make YOLOv8n runnable: {}", e))?;
        Ok(Self {
            plan,
            motion: std::sync::Mutex::new(MotionState {
                mean: vec![0.0; (MOTION_W * MOTION_H) as usize],
                mask: vec![false; (MOTION_W * MOTION_H) as usize],
                initialized: false,
                last_roi_motion: 0.0,
            }),
            motion_sensitivity: std::sync::Mutex::new(0.5),
        })
    }

    /// Hardware Settings tunable (Phase E): 0 = hair-trigger, 1 = only gross motion.
    pub fn set_motion_sensitivity(&self, v: f32) {
        let mut s = self.motion_sensitivity.lock().unwrap_or_else(|e| e.into_inner());
        *s = v.clamp(0.0, 1.0);
    }

    fn motion_thresh(&self) -> f32 {
        let s = self
            .motion_sensitivity
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // sensitivity 0.5 -> 25.0; higher sensitivity -> lower threshold.
        MOTION_BASE_THRESH * (1.5 - *s)
    }

    /// Updates the running background on a grayscale 80x45 copy and returns
    /// the fraction of pixels inside `roi` (original-image coords) that moved.
    fn update_motion(
        &self,
        image: &RgbImage,
        roi_x: f32,
        roi_y: f32,
        roi_w: f32,
        roi_h: f32,
    ) -> f32 {
        let (orig_w, orig_h) = image.dimensions();
        if orig_w == 0 || orig_h == 0 {
            return 0.0;
        }
        let small = image::imageops::resize(image, MOTION_W, MOTION_H, FilterType::Triangle);
        let thresh = self.motion_thresh();
        let mut st = self.motion.lock().unwrap_or_else(|e| e.into_inner());
        let _n = (MOTION_W * MOTION_H) as usize;
        if !st.initialized {
            // Seed background with the first frame (no motion reported).
            for (m, px) in st.mean.iter_mut().zip(small.as_raw().chunks_exact(3)) {
                *m = 0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32;
            }
            st.mask.fill(false);
            st.initialized = true;
            st.last_roi_motion = 0.0;
            return 0.0;
        }
        let sx = MOTION_W as f32 / orig_w as f32;
        let sy = MOTION_H as f32 / orig_h as f32;
        let mut moving = 0usize;
        let mut roi_total = 0usize;
        let mut roi_moving = 0usize;
        for y in 0..MOTION_H {
            for x in 0..MOTION_W {
                let i = (y * MOTION_W + x) as usize;
                let px = &small.as_raw()[i * 3..i * 3 + 3];
                let g = 0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32;
                let diff = (g - st.mean[i]).abs();
                let is_moving = diff > thresh;
                st.mask[i] = is_moving;
                if is_moving {
                    moving += 1;
                }
                // ROI test in small-frame coords.
                let fx = x as f32 / sx;
                let fy = y as f32 / sy;
                if fx >= roi_x && fx <= roi_x + roi_w && fy >= roi_y && fy <= roi_y + roi_h {
                    roi_total += 1;
                    if is_moving {
                        roi_moving += 1;
                    }
                }
                // Adapt background toward current frame (slow: static posters
                // fade into the background, walking people stay foreground).
                st.mean[i] += MOTION_ALPHA * (g - st.mean[i]);
            }
        }
        let _ = moving;
        let ratio = if roi_total > 0 {
            roi_moving as f32 / roi_total as f32
        } else {
            0.0
        };
        st.last_roi_motion = ratio;
        ratio
    }

    /// Fraction of a box (original-image coords) covered by motion pixels.
    fn box_motion_fraction(&self, x: f32, y: f32, bw: f32, bh: f32, orig_w: f32, orig_h: f32) -> f32 {
        let st = self.motion.lock().unwrap_or_else(|e| e.into_inner());
        if orig_w <= 0.0 || orig_h <= 0.0 {
            return 0.0;
        }
        let sx = MOTION_W as f32 / orig_w;
        let sy = MOTION_H as f32 / orig_h;
        let x0 = ((x * sx) as i32).clamp(0, MOTION_W as i32 - 1);
        let y0 = ((y * sy) as i32).clamp(0, MOTION_H as i32 - 1);
        let x1 = (((x + bw) * sx) as i32).clamp(0, MOTION_W as i32 - 1);
        let y1 = (((y + bh) * sy) as i32).clamp(0, MOTION_H as i32 - 1);
        if x1 < x0 || y1 < y0 {
            return 0.0;
        }
        let mut total = 0usize;
        let mut moving = 0usize;
        for yy in y0..=y1 {
            for xx in x0..=x1 {
                total += 1;
                if st.mask[(yy * MOTION_W as i32 + xx) as usize] {
                    moving += 1;
                }
            }
        }
        if total == 0 {
            0.0
        } else {
            moving as f32 / total as f32
        }
    }

    /// Detects person bounding boxes (COCO class 0) in `image`, in ORIGINAL
    /// image coordinates.
    pub fn detect_persons(&self, image: &RgbImage) -> Result<Vec<(f32, f32, f32, f32)>, String> {
        let (orig_w, orig_h) = image.dimensions();
        if orig_w == 0 || orig_h == 0 {
            return Ok(Vec::new());
        }

        let resized = image::imageops::resize(
            image,
            YOLO_INPUT_SIZE,
            YOLO_INPUT_SIZE,
            FilterType::Triangle,
        );
        let scale_x = orig_w as f32 / YOLO_INPUT_SIZE as f32;
        let scale_y = orig_h as f32 / YOLO_INPUT_SIZE as f32;

        // Ultralytics' own preprocessing feeds RGB (not BGR) normalized to
        // [0,1] — unlike the OpenCV-authored face models above.
        let input = rgb_to_nchw_tensor(&resized, false, 1.0 / 255.0)?;

        let outputs = self
            .plan
            .run(tvec!(input.into()))
            .map_err(|e| format!("YOLOv8n inference failed: {}", e))?;

        let output = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| e.to_string())?;
        let data = output
            .as_slice()
            .ok_or("non-contiguous YOLOv8n output tensor")?;

        // Layout: [1, 84, 2100], channel-major — data[channel * 2100 + anchor].
        let mut candidates: Vec<((f32, f32, f32, f32), f32)> = Vec::new();
        for a in 0..YOLO_NUM_ANCHORS {
            let cx = data[a];
            let cy = data[YOLO_NUM_ANCHORS + a];
            let w = data[2 * YOLO_NUM_ANCHORS + a];
            let h = data[3 * YOLO_NUM_ANCHORS + a];

            let mut best_score = 0.0f32;
            let mut best_class = 0usize;
            for c in 0..YOLO_NUM_CLASSES {
                let score = data[(4 + c) * YOLO_NUM_ANCHORS + a];
                if score > best_score {
                    best_score = score;
                    best_class = c;
                }
            }

            if best_class != YOLO_PERSON_CLASS_INDEX || best_score < YOLO_CONF_THRESHOLD {
                continue;
            }

            let x1 = cx - w / 2.0;
            let y1 = cy - h / 2.0;
            candidates.push(((x1, y1, w, h), best_score));
        }

        let kept = nms_scored(candidates, YOLO_NMS_THRESHOLD);

        Ok(kept
            .into_iter()
            .map(|(rect, _score)| {
                (
                    rect.0 * scale_x,
                    rect.1 * scale_y,
                    rect.2 * scale_x,
                    rect.3 * scale_y,
                )
            })
            .collect())
    }

    /// Counts how many detected person boxes have their CENTER point inside
    /// the given ROI rectangle, where `roi_*_pct` are percentages (0-100) of
    /// the frame — matching `CameraConfig::roi_x/roi_y/roi_width/roi_height`
    /// in `gympos-shared`, which the existing camera calibration UI already
    /// edits (`desktop/webview/static/js/app.js: saveRoiCalibration`).
    pub fn count_in_roi(
        &self,
        image: &RgbImage,
        roi_x_pct: f32,
        roi_y_pct: f32,
        roi_w_pct: f32,
        roi_h_pct: f32,
    ) -> Result<usize, String> {
        let boxes = self.detect_persons(image)?;
        let (w, h) = image.dimensions();
        let (w, h) = (w as f32, h as f32);

        let roi_x = (roi_x_pct / 100.0) * w;
        let roi_y = (roi_y_pct / 100.0) * h;
        let roi_w = (roi_w_pct / 100.0) * w;
        let roi_h = (roi_h_pct / 100.0) * h;

        let count = boxes
            .iter()
            .filter(|&&(x, y, bw, bh)| {
                let cx = x + bw / 2.0;
                let cy = y + bh / 2.0;
                cx >= roi_x && cx <= roi_x + roi_w && cy >= roi_y && cy <= roi_y + roi_h
            })
            .count();

        Ok(count)
    }

    /// Counts and locates detected person boxes inside the ROI rectangle,
    /// fused with MOG-lite motion: each box carries `moving` (fraction of
    /// box pixels in motion) and the result carries `motion_in_roi`
    /// (fraction of ROI pixels in motion). A box counts toward `count` when
    /// its center is inside the ROI or it intersects the ROI (locked rule).
    pub fn count_and_locate_in_roi(
        &self,
        image: &RgbImage,
        roi_x_pct: f32,
        roi_y_pct: f32,
        roi_w_pct: f32,
        roi_h_pct: f32,
    ) -> Result<(usize, Vec<serde_json::Value>, f32), String> {
        let boxes = self.detect_persons(image)?;
        let (w, h) = image.dimensions();
        let (w, h) = (w as f32, h as f32);

        let roi_x = (roi_x_pct / 100.0) * w;
        let roi_y = (roi_y_pct / 100.0) * h;
        let roi_w = (roi_w_pct / 100.0) * w;
        let roi_h = (roi_h_pct / 100.0) * h;

        let motion_in_roi = self.update_motion(image, roi_x, roi_y, roi_w, roi_h);

        let mut in_roi = Vec::new();
        for (x, y, bw, bh) in boxes {
            let cx = x + bw / 2.0;
            let cy = y + bh / 2.0;
            let center_in = cx >= roi_x && cx <= roi_x + roi_w && cy >= roi_y && cy <= roi_y + roi_h;
            let intersects = x < roi_x + roi_w && x + bw > roi_x && y < roi_y + roi_h && y + bh > roi_y;
            if center_in || intersects {
                let moving = self.box_motion_fraction(x, y, bw, bh, w, h);
                in_roi.push(serde_json::json!({
                    "x": x, "y": y, "w": bw, "h": bh,
                    "cx": cx, "cy": cy,
                    "moving": moving
                }));
            }
        }

        let count = in_roi.len();
        Ok((count, in_roi, motion_in_roi))
    }
}

/// Greedy IoU-threshold NMS over (rect, score) pairs — the same algorithm as
/// `nms()`/`iou()` above, but decoupled from `DetectedFace` so it can be
/// reused for the person detector's plain rect+score candidates.
fn nms_scored(
    mut boxes: Vec<((f32, f32, f32, f32), f32)>,
    iou_threshold: f32,
) -> Vec<((f32, f32, f32, f32), f32)> {
    boxes.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut kept: Vec<((f32, f32, f32, f32), f32)> = Vec::new();
    'outer: for candidate in boxes {
        for &(kept_rect, _) in &kept {
            if iou(candidate.0, kept_rect) > iou_threshold {
                continue 'outer;
            }
        }
        kept.push(candidate);
    }
    kept
}

/// Converts an RGB image into an NCHW `[1,3,H,W]` f32 tensor.
/// `swap_to_bgr` replicates `swapRB` semantics for models trained on
/// OpenCV's default BGR Mat order. `scale` is a multiplier applied to each
/// raw 0-255 pixel value: use `1.0` to match `cv::dnn::blobFromImage`'s
/// default `scalefactor=1` (used by the face models), or `1.0/255.0` for
/// Ultralytics YOLO models, which are trained on `[0,1]`-normalized input.
fn rgb_to_nchw_tensor(img: &RgbImage, swap_to_bgr: bool, scale: f32) -> Result<Tensor, String> {
    let (w, h) = img.dimensions();
    let mut data = vec![0f32; 3 * h as usize * w as usize];
    let plane = (h * w) as usize;
    for (x, y, px) in img.enumerate_pixels() {
        let Rgb([r, g, b]) = *px;
        let (c0, c1, c2) = if swap_to_bgr { (b, g, r) } else { (r, g, b) };
        let offset = (y as usize) * (w as usize) + x as usize;
        data[offset] = c0 as f32 * scale;
        data[plane + offset] = c1 as f32 * scale;
        data[2 * plane + offset] = c2 as f32 * scale;
    }
    Tensor::from_shape(&[1, 3, h as usize, w as usize], &data).map_err(|e| e.to_string())
}

/// Greedy IoU-threshold NMS, sorted by descending score (mirrors
/// `cv::dnn::NMSBoxes` behavior used by YuNet's post-processing).
fn nms(mut faces: Vec<DetectedFace>, iou_threshold: f32) -> Vec<DetectedFace> {
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<DetectedFace> = Vec::new();
    'outer: for face in faces {
        for k in &kept {
            if iou(face.rect, k.rect) > iou_threshold {
                continue 'outer;
            }
        }
        kept.push(face);
    }
    kept
}

fn iou(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> f32 {
    let (ax1, ay1, aw, ah) = a;
    let (bx1, by1, bw, bh) = b;
    let (ax2, ay2) = (ax1 + aw, ay1 + ah);
    let (bx2, by2) = (bx1 + bw, by1 + bh);

    let ix1 = ax1.max(bx1);
    let iy1 = ay1.max(by1);
    let ix2 = ax2.min(bx2);
    let iy2 = ay2.min(by2);

    let iw = (ix2 - ix1).max(0.0);
    let ih = (iy2 - iy1).max(0.0);
    let intersection = iw * ih;
    let union = (aw * ah) + (bw * bh) - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// Fits a least-squares similarity transform (uniform scale + rotation +
/// translation, no reflection) mapping `src` 5-point landmarks to the
/// canonical [`REFERENCE_LANDMARKS_112`] points. Returns a 2x3 matrix
/// `[[a, -b, tx], [b, a, ty]]` such that `dst = M * src + t`.
///
/// This is the closed-form least-squares solution to the same problem
/// OpenCV solves via a general Umeyama/SVD approach in
/// `FaceRecognizerSF::getSimilarityTransformMatrix`; the two are
/// mathematically equivalent whenever no mirror-reflection correction is
/// needed, which is always true for genuine (non-flipped) face landmarks.
/// See `tests::similarity_transform_recovers_known_transform` for a
/// self-contained correctness check.
fn similarity_transform(src: &[[f32; 2]; 5]) -> [[f64; 3]; 2] {
    let dst = REFERENCE_LANDMARKS_112;

    let mean_src = mean_point(src);
    let mean_dst = mean_point(&dst);

    let mut sxx = 0.0f64; // sum(x'^2 + y'^2) over src
    let mut sxy = 0.0f64; // sum(x'*X' + y'*Y')
    let mut syx = 0.0f64; // sum(x'*Y' - y'*X')

    for i in 0..5 {
        let sx = src[i][0] as f64 - mean_src.0;
        let sy = src[i][1] as f64 - mean_src.1;
        let dx = dst[i][0] as f64 - mean_dst.0;
        let dy = dst[i][1] as f64 - mean_dst.1;

        sxx += sx * sx + sy * sy;
        sxy += sx * dx + sy * dy;
        syx += sx * dy - sy * dx;
    }

    // Degenerate (all points coincide) - fall back to identity to avoid
    // division by zero; callers get an unmodified center crop.
    if sxx.abs() < 1e-9 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    }

    let a = sxy / sxx;
    let b = syx / sxx;
    let tx = mean_dst.0 - a * mean_src.0 + b * mean_src.1;
    let ty = mean_dst.1 - b * mean_src.0 - a * mean_src.1;

    [[a, -b, tx], [b, a, ty]]
}

fn mean_point(points: &[[f32; 2]; 5]) -> (f64, f64) {
    let sx: f64 = points.iter().map(|p| p[0] as f64).sum();
    let sy: f64 = points.iter().map(|p| p[1] as f64).sum();
    (sx / 5.0, sy / 5.0)
}

/// Applies the forward transform's INVERSE to sample `src` into a new
/// 112x112 image (matches `cv::warpAffine(src, dst, M, size, INTER_LINEAR)`
/// semantics: `M` maps src->dst, so filling dst requires the inverse map).
/// Out-of-bounds samples are filled with black, matching OpenCV's default
/// `BORDER_CONSTANT` behavior.
fn warp_affine_112(src: &RgbImage, transform: &[[f64; 3]; 2]) -> RgbImage {
    let a = transform[0][0];
    let b = transform[1][0];
    let tx = transform[0][2];
    let ty = transform[1][2];
    let det = a * a + b * b;

    let mut out = RgbImage::new(REC_INPUT_SIZE, REC_INPUT_SIZE);
    if det.abs() < 1e-12 {
        return out;
    }

    let (src_w, src_h) = src.dimensions();
    for oy in 0..REC_INPUT_SIZE {
        for ox in 0..REC_INPUT_SIZE {
            let dx = ox as f64 - tx;
            let dy = oy as f64 - ty;
            let sx = (a * dx + b * dy) / det;
            let sy = (-b * dx + a * dy) / det;

            out.put_pixel(ox, oy, bilinear_sample(src, sx, sy, src_w, src_h));
        }
    }
    out
}

fn bilinear_sample(img: &RgbImage, x: f64, y: f64, w: u32, h: u32) -> Rgb<u8> {
    if x < 0.0 || y < 0.0 || x >= (w as f64 - 1.0).max(0.0) || y >= (h as f64 - 1.0).max(0.0) {
        // Allow exact edge pixels; otherwise treat as out-of-bounds (black).
        if x < -0.5 || y < -0.5 || x > w as f64 - 0.5 || y > h as f64 - 0.5 {
            return Rgb([0, 0, 0]);
        }
    }

    let x0 = x.floor().clamp(0.0, (w as i64 - 1).max(0) as f64) as u32;
    let y0 = y.floor().clamp(0.0, (h as i64 - 1).max(0) as f64) as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = (x - x0 as f64).clamp(0.0, 1.0);
    let fy = (y - y0 as f64).clamp(0.0, 1.0);

    let p00 = img.get_pixel(x0, y0).0;
    let p10 = img.get_pixel(x1, y0).0;
    let p01 = img.get_pixel(x0, y1).0;
    let p11 = img.get_pixel(x1, y1).0;

    let mut out = [0u8; 3];
    for c in 0..3 {
        let top = p00[c] as f64 * (1.0 - fx) + p10[c] as f64 * fx;
        let bottom = p01[c] as f64 * (1.0 - fx) + p11[c] as f64 * fx;
        out[c] = (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
    }
    Rgb(out)
}

/// Decodes a base64 (optionally `data:image/...;base64,`-prefixed) image
/// string into an RGB image, as sent by the webview from a `<canvas>` frame
/// grab.
pub fn decode_base64_image(data_url: &str) -> Result<RgbImage, String> {
    let b64_part = data_url.split(',').next_back().unwrap_or(data_url);
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64_part.trim())
        .map_err(|e| format!("Invalid base64 image data: {}", e))?;
    let img: DynamicImage =
        image::load_from_memory(&bytes).map_err(|e| format!("Unsupported/corrupt image: {}", e))?;
    Ok(img.to_rgb8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_transform_recovers_known_transform() {
        // Construct src points as a KNOWN scale/rotation/translation of the
        // canonical reference points, then verify the fitted transform
        // reproduces that exact mapping (round-trip through the algorithm).
        let true_scale = 0.8_f64;
        let true_theta = 0.15_f64; // radians
        let true_tx = 12.0_f64;
        let true_ty = -5.0_f64;
        let ca = true_scale * true_theta.cos();
        let cb = true_scale * true_theta.sin();

        // Forward-generate src from dst via the INVERSE of the transform we
        // want `similarity_transform` to recover (so that src -> dst under
        // the algorithm matches [ca,-cb,tx;cb,ca,ty]).
        let mut src = [[0.0f32; 2]; 5];
        let det = ca * ca + cb * cb;
        for i in 0..5 {
            let dx = REFERENCE_LANDMARKS_112[i][0] as f64 - true_tx;
            let dy = REFERENCE_LANDMARKS_112[i][1] as f64 - true_ty;
            let sx = (ca * dx + cb * dy) / det;
            let sy = (-cb * dx + ca * dy) / det;
            src[i] = [sx as f32, sy as f32];
        }

        let m = similarity_transform(&src);

        assert!(
            (m[0][0] - ca).abs() < 1e-6,
            "a mismatch: {} vs {}",
            m[0][0],
            ca
        );
        assert!(
            (m[1][0] - cb).abs() < 1e-6,
            "b mismatch: {} vs {}",
            m[1][0],
            cb
        );
        assert!(
            (m[0][2] - true_tx).abs() < 1e-4,
            "tx mismatch: {} vs {}",
            m[0][2],
            true_tx
        );
        assert!(
            (m[1][2] - true_ty).abs() < 1e-4,
            "ty mismatch: {} vs {}",
            m[1][2],
            true_ty
        );

        // And check it actually maps src -> the reference points.
        for i in 0..5 {
            let x = m[0][0] * src[i][0] as f64 + m[0][1] * src[i][1] as f64 + m[0][2];
            let y = m[1][0] * src[i][0] as f64 + m[1][1] * src[i][1] as f64 + m[1][2];
            assert!((x - REFERENCE_LANDMARKS_112[i][0] as f64).abs() < 1e-3);
            assert!((y - REFERENCE_LANDMARKS_112[i][1] as f64).abs() < 1e-3);
        }
    }

    #[test]
    fn nms_suppresses_overlapping_lower_score_boxes() {
        let faces = vec![
            DetectedFace {
                rect: (10.0, 10.0, 50.0, 50.0),
                landmarks: [[0.0; 2]; 5],
                score: 0.95,
            },
            // Heavily overlapping, lower score -> should be suppressed.
            DetectedFace {
                rect: (12.0, 12.0, 50.0, 50.0),
                landmarks: [[0.0; 2]; 5],
                score: 0.70,
            },
            // Disjoint box -> should survive.
            DetectedFace {
                rect: (500.0, 500.0, 40.0, 40.0),
                landmarks: [[0.0; 2]; 5],
                score: 0.65,
            },
        ];
        let kept = nms(faces, 0.3);
        assert_eq!(kept.len(), 2);
        assert!((kept[0].score - 0.95).abs() < 1e-6);
    }

    #[test]
    fn iou_identical_boxes_is_one() {
        let a = (0.0, 0.0, 10.0, 10.0);
        assert!((iou(a, a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn iou_disjoint_boxes_is_zero() {
        let a = (0.0, 0.0, 10.0, 10.0);
        let b = (100.0, 100.0, 10.0, 10.0);
        assert_eq!(iou(a, b), 0.0);
    }

    #[test]
    fn bilinear_sample_out_of_bounds_is_black() {
        let img = RgbImage::from_pixel(10, 10, Rgb([200, 100, 50]));
        assert_eq!(bilinear_sample(&img, -5.0, -5.0, 10, 10), Rgb([0, 0, 0]));
        assert_eq!(bilinear_sample(&img, 5.0, 5.0, 10, 10), Rgb([200, 100, 50]));
    }

    #[test]
    fn real_onnx_models_load_and_run() {
        // `cargo test` runs with cwd = the package root (`desktop/src-tauri`).
        let dir = std::path::Path::new("../models");
        if !dir.join("face_detection_yunet_2023mar.onnx").is_file() {
            eprintln!("skipping: models not found relative to test working directory");
            return;
        }
        let engine = FaceEngine::load(dir).expect("models should load and optimize");

        let blank = RgbImage::from_pixel(480, 640, Rgb([128, 128, 128]));
        let result = engine
            .detect_largest_face(&blank)
            .expect("detector should run without error");
        assert!(
            result.is_none(),
            "a blank gray frame should not contain a detectable face"
        );
    }

    #[test]
    fn real_yolo_model_loads_and_runs() {
        let dir = std::path::Path::new("../models");
        if !dir.join("yolov8n.onnx").is_file() {
            eprintln!("skipping: yolov8n.onnx not found relative to test working directory");
            return;
        }
        let counter = PersonCounter::load(dir).expect("yolov8n.onnx should load and optimize");

        let blank = RgbImage::from_pixel(480, 640, Rgb([128, 128, 128]));
        let boxes = counter
            .detect_persons(&blank)
            .expect("person detector should run without error");
        assert!(
            boxes.is_empty(),
            "a blank gray frame should not contain any detected persons, got {:?}",
            boxes
        );

        let count = counter
            .count_in_roi(&blank, 20.0, 20.0, 60.0, 60.0)
            .expect("roi count should run without error");
        assert_eq!(count, 0);
    }

    #[test]
    fn motion_background_static_then_changed() {
        let dir = std::path::Path::new("../models");
        if !dir.join("yolov8n.onnx").is_file() {
            eprintln!("skipping: yolov8n.onnx not found relative to test working directory");
            return;
        }
        let counter = PersonCounter::load(dir).expect("yolov8n.onnx should load and optimize");

        // First frame seeds the background: no motion reported.
        let blank = RgbImage::from_pixel(320, 240, Rgb([128, 128, 128]));
        let m0 = counter.update_motion(&blank, 0.0, 0.0, 320.0, 240.0);
        assert_eq!(m0, 0.0, "first frame seeds background, motion must be 0");
        // Identical second frame: still (near-)zero motion.
        let m1 = counter.update_motion(&blank, 0.0, 0.0, 320.0, 240.0);
        assert!(
            m1 < 0.01,
            "static frame must report near-zero motion, got {}",
            m1
        );
        // Half-white frame: large changed area must register motion.
        let mut changed = blank.clone();
        for y in 0..240 {
            for x in 0..160 {
                changed.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let m2 = counter.update_motion(&changed, 0.0, 0.0, 320.0, 240.0);
        assert!(
            m2 > 0.2,
            "half-changed frame must register strong motion, got {}",
            m2
        );
        // Boxes carry a moving fraction in [0,1].
        let (_count, boxes, motion) = counter
            .count_and_locate_in_roi(&changed, 0.0, 0.0, 100.0, 100.0)
            .expect("locate should run");
        assert!((0.0..=1.0).contains(&motion), "motion ratio in range");
        for b in &boxes {
            let mv = b.get("moving").and_then(|v| v.as_f64()).unwrap_or(-1.0);
            assert!(
                (0.0..=1.0).contains(&mv),
                "per-box moving fraction in range"
            );
        }
    }

    #[test]
    fn nms_scored_suppresses_overlapping_lower_score_boxes() {
        let boxes = vec![
            ((10.0, 10.0, 50.0, 50.0), 0.9),
            ((12.0, 12.0, 50.0, 50.0), 0.6),
            ((500.0, 500.0, 40.0, 40.0), 0.5),
        ];
        let kept = nms_scored(boxes, 0.3);
        assert_eq!(kept.len(), 2);
        assert!((kept[0].1 - 0.9).abs() < 1e-6);
    }

    #[test]
    fn insightface_normalization_maps_pixel_range_to_minus_one_to_one() {
        // InsightFace spec: (x - 127.5) / 127.5 on RGB.
        let img = RgbImage::from_pixel(1, 1, Rgb([255, 0, 128]));
        let t = rgb_to_nchw_tensor_normalized(&img, false, 127.5, 127.5)
            .expect("tensor build should succeed");
        let v = t
            .to_array_view::<f32>()
            .expect("tensor should be f32")
            .as_slice()
            .expect("contiguous")
            .to_vec();
        // NCHW plane order: R, G, B.
        assert!((v[0] - 1.0).abs() < 1e-6, "255 -> 1.0, got {}", v[0]);
        assert!((v[1] + 1.0).abs() < 1e-6, "0 -> -1.0, got {}", v[1]);
        assert!(
            (v[2] - (128.0 - 127.5) / 127.5).abs() < 1e-6,
            "128 -> ~0.004, got {}",
            v[2]
        );
    }

    #[test]
    fn real_arcface_model_loads_and_runs() {
        let dir = std::path::Path::new("../models");
        if !dir.join(ARCFACE_MODEL_FILENAME).is_file() {
            eprintln!("skipping: {} not found relative to test working directory", ARCFACE_MODEL_FILENAME);
            return;
        }
        let engine = FaceEngine::load(dir).expect("models should load and optimize");
        assert!(
            engine.arcface.is_some(),
            "ArcFace plan should load when the model file is present"
        );
        assert_eq!(engine.embedding_dim(), ARCFACE_EMBEDDING_DIM);

        // A uniform mid-gray aligned crop is not a face, but the recognizer
        // must still return a well-formed 512-d finite embedding for it.
        let crop = RgbImage::from_pixel(112, 112, Rgb([128, 128, 128]));
        let face = DetectedFace {
            rect: (0.0, 0.0, 112.0, 112.0),
            landmarks: REFERENCE_LANDMARKS_112,
            score: 1.0,
        };
        let embedding = engine
            .extract_embedding_arcface(&crop, &face)
            .expect("arcface inference should run without error");
        assert_eq!(embedding.len(), ARCFACE_EMBEDDING_DIM);
        assert!(
            embedding.iter().all(|x| x.is_finite()),
            "embedding must be finite"
        );
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(norm > 1e-6, "embedding must be non-degenerate");
    }
}

