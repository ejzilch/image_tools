use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use chrono::Local;
use dashmap::DashMap;
use fast_image_resize::{self as fir, Resizer};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use tauri::Emitter;

// Global statics
static FONT_CACHE: OnceLock<DashMap<String, FontVec>> = OnceLock::new();

static SUPPORTED_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "tiff", "tif", "heic", "heif"];

// Public types
pub struct ProcessingState {
    pub cancel_flag: Arc<AtomicBool>,
    pub output_files: Arc<Mutex<Vec<PathBuf>>>,
}

#[derive(serde::Serialize, Clone)]
struct ProgressPayload {
    current: usize,
    total: usize,
    file: String,
    success: bool,
    error: Option<String>,
}

#[derive(serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WatermarkOptions {
    pub text: String,
    pub font_name: String,
    pub opacity: f32,
    pub color: [u8; 3],
    pub bold: bool,
    pub position: String,
    pub font_size: f32,
}

#[derive(serde::Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum OutputFormat {
    Original,
    Jpeg,
    Webp,
}

impl OutputFormat {
    fn from_str(s: &str) -> Self {
        match s {
            "jpeg" | "jpg" => OutputFormat::Jpeg,
            "webp" => OutputFormat::Webp,
            _ => OutputFormat::Original,
        }
    }
}

trait ImageOp: Send + Sync {
    fn apply(&self, img: image::RgbImage) -> Result<image::RgbImage, String>;
}

struct ResizeOp {
    dst_w: u32,
    dst_h: u32,
}

impl ImageOp for ResizeOp {
    fn apply(&self, rgb: image::RgbImage) -> Result<image::RgbImage, String> {
        resize_rgb(&rgb, self.dst_w, self.dst_h)
    }
}

struct WatermarkOp {
    options: WatermarkOptions,
}

impl ImageOp for WatermarkOp {
    fn apply(&self, rgb: image::RgbImage) -> Result<image::RgbImage, String> {
        apply_watermark(rgb, &self.options)
    }
}

struct ProcessOptions {
    ops: Vec<Box<dyn ImageOp>>,
    compress_target: Option<u64>,
    rename_stem: Option<String>,
    out_suffix: String,
    output_format: OutputFormat,
}

// cancel checkpoint helper
#[inline]
fn check_cancel(flag: &AtomicBool) -> Result<(), String> {
    if flag.load(Ordering::Relaxed) {
        Err("cancelled".into())
    } else {
        Ok(())
    }
}

struct LoadedImage {
    rgb: image::RgbImage,
    // 供「不需要重壓縮」路徑直接複製（避免重新 encode）
    raw_bytes: Vec<u8>,
    ext: String,
}

fn font_cache() -> &'static DashMap<String, FontVec> {
    FONT_CACHE.get_or_init(DashMap::new)
}

fn get_font(name: &str) -> Result<dashmap::mapref::one::Ref<'static, String, FontVec>, String> {
    let cache = font_cache();

    if !cache.contains_key(name) {
        let data: &[u8] = match name {
            "notosanstc" => include_bytes!("../../public/fonts/NotoSansTC-VariableFont_wght.ttf"),
            "chenyuluoyan" => include_bytes!("../../public/fonts/ChenYuluoyan-2.0-Thin.ttf"),
            "dancingscript" => {
                include_bytes!("../../public/fonts/DancingScript-VariableFont_wght.ttf")
            }
            _ => return Err(format!("未知字型: {}", name)),
        };
        let font =
            FontVec::try_from_vec(data.to_vec()).map_err(|_| format!("字型載入失敗: {}", name))?;
        cache.insert(name.to_string(), font);
    }

    cache.get(name).ok_or_else(|| "字型取得失敗".to_string())
}

fn load_image(img_path: &Path) -> Result<LoadedImage, String> {
    let raw_bytes = fs::read(img_path).map_err(|e| format!("讀取失敗 {:?}: {}", img_path, e))?;
    let ext = img_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let rgb = match ext.as_str() {
        "heic" | "heif" => decode_heic(img_path)?,
        _ => decode_to_rgb(&raw_bytes, &ext)?,
    };

    Ok(LoadedImage {
        rgb,
        raw_bytes,
        ext,
    })
}

fn process_image(
    rgb: image::RgbImage,
    ops: &[Box<dyn ImageOp>],
    cancel_flag: &AtomicBool,
) -> Result<image::RgbImage, String> {
    let mut img = rgb;
    for op in ops {
        check_cancel(cancel_flag)?;
        img = op.apply(img)?;
    }
    Ok(img)
}

fn save_result(
    rgb: image::RgbImage,
    raw_bytes: &[u8],
    ext: &str,
    out: &Path,
    compress_target: Option<u64>,
    has_ops: bool,
    output_format: &OutputFormat,
) -> Result<PathBuf, String> {
    // 決定實際輸出副檔名
    let out_ext = match output_format {
        OutputFormat::Jpeg => "jpg",
        OutputFormat::Webp => "webp",
        OutputFormat::Original => {
            // HEIC 原格式無法輸出，fallback 到 jpg
            if matches!(ext, "heic" | "heif") {
                "jpg"
            } else {
                ext
            }
        }
    };

    // 如果副檔名需要變更，重建輸出路徑
    let out = if out.extension().and_then(|e| e.to_str()).unwrap_or("") != out_ext {
        out.with_extension(out_ext)
    } else {
        out.to_path_buf()
    };

    if let Some(target) = compress_target {
        let current_size = raw_bytes.len() as u64;

        // 若原始已經小於目標且不需要 ops 和格式轉換
        if current_size <= target && !has_ops && matches!(output_format, OutputFormat::Original) {
            fs::write(&out, raw_bytes).map_err(|e| format!("複製失敗: {}", e))?;
            return Ok(out);
        }

        let compressed = compress_to_target(&rgb, current_size, target, out_ext)?;

        fs::write(&out, compressed).map_err(|e| format!("無法儲存: {}", e))?;
        return Ok(out);
    }

    // 無壓縮目標
    match out_ext {
        "webp" => {
            let encoded = encode_webp(&rgb, 90)?;
            fs::write(&out, encoded).map_err(|e| format!("無法儲存: {}", e))?;
        }
        "jpg" | "jpeg" => {
            if has_ops || !matches!(output_format, OutputFormat::Original) {
                let encoded = encode_jpeg_mozjpeg(&rgb, 92)?;
                fs::write(&out, encoded).map_err(|e| format!("無法儲存: {}", e))?;
            } else {
                fs::write(&out, raw_bytes).map_err(|e| format!("複製失敗: {}", e))?;
            }
        }
        _ => {
            if has_ops {
                write_image(&rgb, &out, out_ext)?;
            } else {
                fs::write(&out, raw_bytes).map_err(|e| format!("複製失敗: {}", e))?;
            }
        }
    }

    Ok(out)
}

fn process_one(
    img_path: &Path,
    opts: &ProcessOptions,
    cancel_flag: &AtomicBool,
) -> Result<PathBuf, String> {
    check_cancel(cancel_flag)?;
    let loaded = load_image(img_path)?;

    let has_ops = !opts.ops.is_empty();
    let rgb = process_image(loaded.rgb, &opts.ops, cancel_flag)?;

    check_cancel(cancel_flag)?;
    let out_dir = ensure_output_dir(img_path, &opts.out_suffix);
    let out = match &opts.rename_stem {
        Some(stem) => out_dir.join(format!("{}.{}", stem, loaded.ext)),
        None => output_path_with_dir(&out_dir, img_path),
    };

    let out_path = save_result(
        rgb,
        &loaded.raw_bytes,
        &loaded.ext,
        &out,
        opts.compress_target,
        has_ops,
        &opts.output_format,
    )?;

    if cancel_flag.load(Ordering::Relaxed) {
        fs::remove_file(&out_path).ok();
        return Err("cancelled".into());
    }

    Ok(out_path)
}

struct BatchResult {
    outputs: Vec<String>,
    output_dirs: Vec<String>,
}

#[derive(serde::Serialize)]
struct CommandResult {
    outputs: Vec<String>,
    output_dirs: Vec<String>,
}

fn run_parallel<F>(
    app: &tauri::AppHandle,
    cancel_flag: Arc<AtomicBool>,
    output_files: Arc<Mutex<Vec<PathBuf>>>,
    images: &[PathBuf],
    make_opts: F,
) -> BatchResult
where
    F: Fn(usize, &Path) -> ProcessOptions + Send + Sync,
{
    let total = images.len();
    app.emit("progress_total", total).ok();

    // 記錄真正完成的數量
    let completed = std::sync::atomic::AtomicUsize::new(0);

    let results: Vec<(usize, PathBuf, Result<PathBuf, String>)> = images
        .par_iter()
        .enumerate()
        .map(|(idx, img_path)| {
            if cancel_flag.load(Ordering::Relaxed) {
                return (idx, img_path.clone(), Err("cancelled".to_string()));
            }
            let opts = make_opts(idx, img_path);

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                process_one(img_path, &opts, &cancel_flag)
            }))
            .unwrap_or_else(|e| {
                let msg = e
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| e.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "未知錯誤".to_string());
                Err(format!("內部錯誤: {}", msg))
            });

            let file_name = img_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            // fetch_add 回傳的是加之前的值，所以 +1
            let current = completed.fetch_add(1, Ordering::Relaxed) + 1;

            match &result {
                Ok(_) => {
                    app.emit(
                        "progress",
                        ProgressPayload {
                            current,
                            total,
                            file: file_name,
                            success: true,
                            error: None,
                        },
                    )
                    .ok();
                }
                Err(e) if e == "cancelled" => { /* 執行取消就跳過 */ }
                Err(e) => {
                    app.emit(
                        "progress",
                        ProgressPayload {
                            current,
                            total,
                            file: file_name,
                            success: false,
                            error: Some(e.clone()),
                        },
                    )
                    .ok();
                }
            }

            (idx, img_path.clone(), result)
        })
        .collect();

    // collect 後只負責收集路徑和錯誤
    let mut success_paths: Vec<PathBuf> = Vec::with_capacity(results.len());
    let mut outputs: Vec<String> = Vec::with_capacity(results.len());

    for (_, _, result) in results {
        match result {
            Ok(path) => {
                success_paths.push(path.clone());
                outputs.push(path.to_string_lossy().to_string());
            }
            Err(_) => { /* 已透過 progress event 處理 */ }
        }
    }

    // collect 完成後，檢查是否已取消
    if cancel_flag.load(Ordering::Relaxed) {
        for path in &success_paths {
            fs::remove_file(path).ok();
        }
        return BatchResult {
            outputs: vec![],
            output_dirs: vec![],
        };
    }

    // 從 success_paths 取不重複的輸出資料夾
    let output_dirs: Vec<String> = {
        let mut dirs: std::collections::HashSet<String> = std::collections::HashSet::new();
        for path in &success_paths {
            if let Some(parent) = path.parent() {
                dirs.insert(parent.to_string_lossy().to_string());
            }
        }
        dirs.into_iter().collect()
    };

    *output_files.lock().expect("poisoned mutex") = success_paths;

    BatchResult {
        outputs,
        output_dirs,
    }
}

#[tauri::command]
async fn shrink_image(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProcessingState>,
    inputs: Vec<String>,
    shrink_mode: String,
    width: u32,
    height: u32,
    ratio: f32,
    watermark: Option<WatermarkOptions>,
    output_format: String,
) -> Result<CommandResult, String> {
    state.cancel_flag.store(false, Ordering::Relaxed);
    state.output_files.lock().expect("poisoned mutex").clear();

    let cancel_flag = state.cancel_flag.clone();
    let output_files = state.output_files.clone();
    let fmt = OutputFormat::from_str(&output_format);

    tokio::task::spawn_blocking(move || -> Result<CommandResult, String> {
        let images = collect_images_from_inputs(&inputs);
        if images.is_empty() {
            return Ok(CommandResult {
                outputs: vec![],
                output_dirs: vec![],
            });
        }

        // 預先建輸出目錄，避免 par_iter 時競爭
        let _dirs: HashSet<PathBuf> = images
            .iter()
            .map(|p| ensure_output_dir(p, "shrink"))
            .collect();

        let batch = run_parallel(
            &app,
            cancel_flag,
            output_files,
            &images,
            |_idx, img_path| {
                let (dst_w, dst_h) = if shrink_mode == "dimension" {
                    (width, height)
                } else {
                    // image_dimensions 只讀 header
                    // 避免 decode 兩次，跟不必要的 decode
                    let (w, h) = image::image_dimensions(img_path).unwrap_or((1, 1));
                    (
                        (w as f32 * ratio / 100.0).max(1.0) as u32,
                        (h as f32 * ratio / 100.0).max(1.0) as u32,
                    )
                };

                let mut ops: Vec<Box<dyn ImageOp>> = vec![Box::new(ResizeOp { dst_w, dst_h })];
                if let Some(ref wm) = watermark {
                    ops.push(Box::new(WatermarkOp {
                        options: wm.clone(),
                    }));
                }

                ProcessOptions {
                    ops,
                    compress_target: None,
                    rename_stem: None,
                    out_suffix: "shrink".to_string(),
                    output_format: fmt.clone(),
                }
            },
        );

        Ok(CommandResult {
            outputs: batch.outputs,
            output_dirs: batch.output_dirs,
        })
    })
    .await
    .map_err(|e| format!("執行緒錯誤: {}", e))?
}

#[tauri::command]
async fn compress_image(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProcessingState>,
    inputs: Vec<String>,
    target_bytes: u64,
    watermark: Option<WatermarkOptions>,
    output_format: String,
) -> Result<CommandResult, String> {
    state.cancel_flag.store(false, Ordering::Relaxed);
    state.output_files.lock().expect("poisoned mutex").clear();

    let cancel_flag = state.cancel_flag.clone();
    let output_files = state.output_files.clone();
    let fmt = OutputFormat::from_str(&output_format);

    tokio::task::spawn_blocking(move || -> Result<CommandResult, String> {
        let images = collect_images_from_inputs(&inputs);
        if images.is_empty() {
            return Ok(CommandResult {
                outputs: vec![],
                output_dirs: vec![],
            });
        }

        let batch = run_parallel(
            &app,
            cancel_flag,
            output_files,
            &images,
            |_idx, _img_path| {
                let mut ops: Vec<Box<dyn ImageOp>> = vec![];
                if let Some(ref wm) = watermark {
                    ops.push(Box::new(WatermarkOp {
                        options: wm.clone(),
                    }));
                }
                ProcessOptions {
                    ops,
                    compress_target: Some(target_bytes),
                    rename_stem: None,
                    out_suffix: "compress".to_string(),
                    output_format: fmt.clone(),
                }
            },
        );

        Ok(CommandResult {
            outputs: batch.outputs,
            output_dirs: batch.output_dirs,
        })
    })
    .await
    .map_err(|e| format!("執行緒錯誤: {}", e))?
}

#[tauri::command]
async fn watermark_only(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProcessingState>,
    inputs: Vec<String>,
    watermark: WatermarkOptions,
    output_format: String,
) -> Result<CommandResult, String> {
    state.cancel_flag.store(false, Ordering::Relaxed);
    state
        .output_files
        .lock()
        .expect("poisoned mutex from watermark_only.")
        .clear();

    let cancel_flag = state.cancel_flag.clone();
    let output_files = state.output_files.clone();
    let fmt = OutputFormat::from_str(&output_format);

    tokio::task::spawn_blocking(move || -> Result<CommandResult, String> {
        let images = collect_images_from_inputs(&inputs);
        if images.is_empty() {
            return Ok(CommandResult {
                outputs: vec![],
                output_dirs: vec![],
            });
        }

        let batch = run_parallel(
            &app,
            cancel_flag,
            output_files,
            &images,
            |_idx, _img_path| ProcessOptions {
                ops: vec![Box::new(WatermarkOp {
                    options: watermark.clone(),
                })],
                compress_target: None,
                rename_stem: None,
                out_suffix: "watermark".to_string(),
                output_format: fmt.clone(),
            },
        );

        Ok(CommandResult {
            outputs: batch.outputs,
            output_dirs: batch.output_dirs,
        })
    })
    .await
    .map_err(|e| format!("執行緒錯誤: {}", e))?
}

#[tauri::command]
async fn rename_images(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProcessingState>,
    inputs: Vec<String>,
    custom_text: String,
    rename_mode: String,
    watermark: Option<WatermarkOptions>,
    output_format: String,
) -> Result<CommandResult, String> {
    state.cancel_flag.store(false, Ordering::Relaxed);
    state
        .output_files
        .lock()
        .expect("poisoned mutex from rename_images.")
        .clear();

    let cancel_flag = state.cancel_flag.clone();
    let output_files = state.output_files.clone();
    let fmt = OutputFormat::from_str(&output_format);

    tokio::task::spawn_blocking(move || -> Result<CommandResult, String> {
        let images = collect_images_from_inputs(&inputs);
        if images.is_empty() {
            return Ok(CommandResult {
                outputs: vec![],
                output_dirs: vec![],
            });
        }

        let date_str = Local::now().format("%Y-%m-%d").to_string();

        let batch = run_parallel(
            &app,
            cancel_flag,
            output_files,
            &images,
            |idx, _img_path| {
                let seq = format!("{:05}", idx + 1);
                let stem = match rename_mode.as_str() {
                    "date_sequence" => format!("{}_{}_{}", custom_text, date_str, seq),
                    _ => format!("{}_{}", custom_text, seq),
                };

                let mut ops: Vec<Box<dyn ImageOp>> = vec![];
                if let Some(ref wm) = watermark {
                    ops.push(Box::new(WatermarkOp {
                        options: wm.clone(),
                    }));
                }

                ProcessOptions {
                    ops,
                    compress_target: None,
                    rename_stem: Some(stem),
                    out_suffix: "rename".to_string(),
                    output_format: fmt.clone(),
                }
            },
        );

        Ok(CommandResult {
            outputs: batch.outputs,
            output_dirs: batch.output_dirs,
        })
    })
    .await
    .map_err(|e| format!("執行緒錯誤: {}", e))?
}

#[tauri::command]
async fn cancel_processing(state: tauri::State<'_, ProcessingState>) -> Result<(), String> {
    state.cancel_flag.store(true, Ordering::Relaxed);
    let files = state
        .output_files
        .lock()
        .expect("poisoned mutex from cancel_processing.")
        .clone();
    for path in &files {
        fs::remove_file(path).ok();
    }
    state.output_files.lock().expect("poisoned mutex").clear();
    Ok(())
}

#[tauri::command]
fn check_output_dir_exists(input_path: String, suffix: String) -> bool {
    let path = Path::new(&input_path);
    let parent = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(Path::new("."))
    };
    parent.join(&suffix).exists()
}

fn ensure_output_dir(img_path: &Path, suffix: &str) -> PathBuf {
    let parent = img_path.parent().unwrap_or(Path::new("."));
    let out_dir = parent.join(suffix);
    fs::create_dir_all(&out_dir).ok();
    out_dir
}

fn output_path_with_dir(out_dir: &Path, img_path: &Path) -> PathBuf {
    let stem = img_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let ext = img_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    out_dir.join(format!("{}.{}", stem, ext))
}

fn collect_images(input: &str) -> Vec<PathBuf> {
    let path = Path::new(input);
    if path.is_dir() {
        fs::read_dir(path)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| SUPPORTED_EXTS.contains(&e.to_lowercase().as_str()))
                    .unwrap_or(false)
            })
            .collect()
    } else {
        vec![path.to_path_buf()]
    }
}

fn collect_images_from_inputs(inputs: &[String]) -> Vec<PathBuf> {
    inputs.iter().flat_map(|i| collect_images(i)).collect()
}

fn decode_to_rgb(bytes: &[u8], ext: &str) -> Result<image::RgbImage, String> {
    if matches!(ext, "jpg" | "jpeg") {
        decode_jpeg_mozjpeg_rgb(bytes)
    } else {
        image::load_from_memory(bytes)
            .map_err(|e| format!("無法開啟: {}", e))
            .map(|d| d.into_rgb8())
    }
}

fn decode_jpeg_mozjpeg_rgb(bytes: &[u8]) -> Result<image::RgbImage, String> {
    let decompress = mozjpeg::Decompress::with_markers(mozjpeg::ALL_MARKERS)
        .from_mem(bytes)
        .map_err(|e| format!("mozjpeg decode 失敗: {}", e))?;

    let mut decompress = decompress
        .rgb()
        .map_err(|e| format!("mozjpeg 轉換 RGB 失敗: {}", e))?;

    let width = decompress.width() as u32;
    let height = decompress.height() as u32;

    let pixels: Vec<u8> = decompress
        .read_scanlines::<u8>()
        .map_err(|e| format!("讀取像素失敗: {}", e))?
        .into_iter()
        .collect();

    decompress
        .finish()
        .map_err(|e| format!("decompress finish 失敗: {}", e))?;

    image::RgbImage::from_raw(width, height, pixels).ok_or("建立圖片失敗".to_string())
}

fn decode_heic(img_path: &Path) -> Result<image::RgbImage, String> {
    // 先寫到 temp jpg，再讀回來
    let tmp = std::env::temp_dir().join(format!(
        "heic_tmp_{}.jpg",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
    ));

    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("sips")
            .args([
                "-s",
                "format",
                "jpeg",
                img_path.to_str().unwrap_or(""),
                "--out",
                tmp.to_str().unwrap_or(""),
            ])
            .status()
            .map_err(|e| format!("sips 執行失敗: {}", e))?;

        if !status.success() {
            return Err("sips 轉換 HEIC 失敗".to_string());
        }
    }

    #[cfg(target_os = "windows")]
    {
        // 嘗試呼叫 magick
        let status = std::process::Command::new("magick")
            .args([img_path.to_str().unwrap_or(""), tmp.to_str().unwrap_or("")])
            .status();

        match status {
            Ok(s) if s.success() => {}
            Ok(_) => return Err("ImageMagick 轉換 HEIC 失敗".to_string()),
            Err(_) => {
                return Err(
                    "無法開啟 HEIC：請先安裝 ImageMagick（https://imagemagick.org）".to_string(),
                )
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        return Err("Linux 不支援 HEIC 輸入".to_string());
    }

    let bytes = fs::read(&tmp).map_err(|e| format!("讀取暫存檔失敗: {}", e))?;
    fs::remove_file(&tmp).ok();
    decode_jpeg_mozjpeg_rgb(&bytes)
}

fn resize_rgb(rgb: &image::RgbImage, dst_w: u32, dst_h: u32) -> Result<image::RgbImage, String> {
    let src = fir::images::Image::from_vec_u8(
        rgb.width(),
        rgb.height(),
        rgb.as_raw().to_vec(),
        fir::PixelType::U8x3,
    )
    .map_err(|e| format!("fir 建立失敗: {}", e))?;

    let mut dst = fir::images::Image::new(dst_w, dst_h, fir::PixelType::U8x3);

    Resizer::new()
        .resize(
            &src,
            &mut dst,
            &fir::ResizeOptions::new()
                .resize_alg(fir::ResizeAlg::Convolution(fir::FilterType::Lanczos3)),
        )
        .map_err(|e| format!("resize 失敗: {}", e))?;

    image::RgbImage::from_raw(dst_w, dst_h, dst.into_vec()).ok_or("resize 轉換失敗".to_string())
}

fn compress_to_target(
    img: &image::RgbImage,
    raw_bytes_len: u64,
    target_bytes: u64,
    out_ext: &str,
) -> Result<Vec<u8>, String> {
    let tolerance = (target_bytes / 10).clamp(100 * 1024, 200 * 1024);
    let mut low: u8 = 1;
    let mut high: u8 = 95;
    let mut best: Vec<u8> = Vec::new();

    // 使用原圖與目標算出參考中間值
    let ratio = target_bytes as f32 / raw_bytes_len as f32;
    let mut mid = (ratio * 95.0).clamp(1.0, 94.0) as u8;

    while low <= high {
        let bytes = match out_ext {
            "webp" => encode_webp(img, mid)?,
            _ => encode_jpeg_mozjpeg(img, mid)?,
        };

        let size = bytes.len() as u64;
        if size.abs_diff(target_bytes) <= tolerance {
            return Ok(bytes);
        }

        if size <= target_bytes {
            best = bytes;
            low = mid + 1;
        } else {
            // 避免 overflow
            high = mid.saturating_sub(1);
        }

        if low > high {
            break;
        }

        mid = low + (high - low) / 2;
    }

    if best.is_empty() {
        best = match out_ext {
            "webp" => encode_webp(img, 1)?,
            _ => encode_jpeg_mozjpeg(img, 1)?,
        };
    }
    Ok(best)
}

fn encode_jpeg_mozjpeg(rgb: &image::RgbImage, quality: u8) -> Result<Vec<u8>, String> {
    let (width, height) = rgb.dimensions();
    let mut compress = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
    compress.set_size(width as usize, height as usize);
    compress.set_quality(quality as f32);

    let mut buf = Vec::new();
    let mut compress = compress
        .start_compress(&mut buf)
        .map_err(|e| format!("mozjpeg 初始化失敗: {}", e))?;

    for row in rgb.as_raw().chunks(width as usize * 3) {
        compress
            .write_scanlines(row)
            .map_err(|e| format!("mozjpeg 寫入失敗: {}", e))?;
    }

    compress
        .finish()
        .map_err(|e| format!("mozjpeg 完成失敗: {}", e))?;
    Ok(buf)
}

fn encode_webp(rgb: &image::RgbImage, quality: u8) -> Result<Vec<u8>, String> {
    let (width, height) = rgb.dimensions();
    let encoder = webp::Encoder::from_rgb(rgb.as_raw(), width, height);
    let mut config = webp::WebPConfig::new().map_err(|_| "WebP config 初始化失敗".to_string())?;
    config.quality = quality as f32;
    let encoded = encoder
        .encode_advanced(&config)
        .map_err(|_| "WebP 編碼失敗".to_string())?;

    Ok(encoded.to_vec())
}

fn write_image(img: &image::RgbImage, out: &Path, ext: &str) -> Result<(), String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let fmt = image::ImageFormat::from_extension(ext).unwrap_or(image::ImageFormat::Jpeg);
    img.write_to(&mut buf, fmt)
        .map_err(|e| format!("編碼失敗: {}", e))?;
    fs::write(out, buf.into_inner()).map_err(|e| format!("無法儲存: {}", e))?;
    Ok(())
}

fn measure_text_width(font: &FontVec, text: &str, scale: PxScale) -> u32 {
    let scaled = font.as_scaled(scale);
    text.chars()
        .map(|c| scaled.h_advance(font.glyph_id(c)))
        .sum::<f32>() as u32
}

fn calc_watermark_position(
    position: &str,
    img_w: u32,
    img_h: u32,
    text_w: u32,
    text_h: u32,
    margin: u32,
) -> (u32, u32) {
    match position {
        "top-left" => (margin, margin),
        "top-right" => (img_w.saturating_sub(text_w + margin), margin),
        "bottom-left" => (margin, img_h.saturating_sub(text_h + margin)),
        _ => (
            img_w.saturating_sub(text_w + margin),
            img_h.saturating_sub(text_h + margin),
        ),
    }
}

fn draw_text_on(
    font: &FontVec,
    img: &mut image::RgbaImage,
    color: image::Rgba<u8>,
    x: i32,
    y: i32,
    scale: PxScale,
    text: &str,
    bold: bool,
) {
    if bold {
        for dx in -1i32..=1 {
            for dy in -1i32..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                imageproc::drawing::draw_text_mut(img, color, x + dx, y + dy, scale, font, text);
            }
        }
    }
    imageproc::drawing::draw_text_mut(img, color, x, y, scale, font, text);
}

fn apply_tiled_watermark(
    font: &FontVec,
    img: &mut image::RgbaImage,
    wm: &WatermarkOptions,
    scale: PxScale,
    color: image::Rgba<u8>,
) {
    let text_w = measure_text_width(font, &wm.text, scale);

    // 計算文字實際高度
    let text_h = {
        let s = font.as_scaled(scale);
        (s.ascent() - s.descent()).ceil() as f32
    };

    let (img_w, img_h) = (img.width(), img.height());
    let row_count = 5i32;

    // 計算每一排之間的間距：(總高度 - 文字高度) / (排數 - 1)
    // row 0 會在 0，最後一 row 會在 img_h - text_h
    let spacing_y = if row_count > 1 {
        (img_h as f32 - text_h) / (row_count as f32 - 1.0)
    } else {
        0.0
    };

    // 水平間距維持原本邏輯，確保左右鋪滿
    let spacing_x = text_w as f32 * 3.0;
    let offset = spacing_x / 2.0;

    for row in 0..row_count {
        let y = (row as f32 * spacing_y) as i32;
        let x_off = if row % 2 == 0 { 0.0 } else { offset };
        let mut x = -(spacing_x as i32) + (x_off as i32);

        while x < img_w as i32 {
            draw_text_on(font, img, color, x, y, scale, &wm.text, wm.bold);
            x += spacing_x as i32;
        }
    }
}

fn apply_diagonal_watermark(
    font: &FontVec,
    img: &mut image::RgbaImage,
    wm: &WatermarkOptions,
    scale: PxScale,
    color: image::Rgba<u8>,
) {
    let (img_w, img_h) = (img.width(), img.height());
    let text_w = measure_text_width(font, &wm.text, scale);
    let text_h = {
        let s = font.as_scaled(scale);
        (s.ascent() - s.descent()).ceil() as u32
    };
    let angle_rad = (img_h as f32 / img_w as f32).atan();
    let diagonal = ((text_w * text_w + text_h * text_h) as f32).sqrt() as u32;
    let canvas_sz = diagonal + 40;

    let mut text_layer = image::RgbaImage::new(canvas_sz, canvas_sz);
    let tx = (canvas_sz / 2).saturating_sub(text_w / 2) as i32;
    let ty = (canvas_sz / 2).saturating_sub(text_h / 2) as i32;
    draw_text_on(
        font,
        &mut text_layer,
        color,
        tx,
        ty,
        scale,
        &wm.text,
        wm.bold,
    );

    let rotated = imageproc::geometric_transformations::rotate_about_center(
        &text_layer,
        -angle_rad,
        imageproc::geometric_transformations::Interpolation::Bilinear,
        image::Rgba([0, 0, 0, 0]),
    );

    let (rw, rh) = (rotated.width() as i64, rotated.height() as i64);
    let spacing_x = (text_w as f32 * 2.6) as i64;
    let spacing_y = (img_h as i64 / 5).max(text_h as i64 * 3);
    let offset = spacing_x / 2;

    let mut row = 0i64;
    let mut y = -rh;
    while y < img_h as i64 + rh {
        let x_off = if row % 2 == 0 { 0 } else { offset };
        let mut x = -rw + x_off;
        while x < img_w as i64 + rw {
            image::imageops::overlay(img, &rotated, x, y);
            x += spacing_x;
        }
        y += spacing_y;
        row += 1;
    }
}

fn apply_watermark(rgb: image::RgbImage, wm: &WatermarkOptions) -> Result<image::RgbImage, String> {
    let font_name = if wm.font_name.is_empty() {
        "notosanstc"
    } else {
        wm.font_name.as_str()
    };

    let font = get_font(font_name)?; // 只取一次，往下傳
    let font = &*font;

    let scale = PxScale::from(wm.font_size * (rgb.width() as f32 / 1000.0));
    let color = image::Rgba([
        wm.color[0],
        wm.color[1],
        wm.color[2],
        (wm.opacity * 255.0) as u8,
    ]);
    let mut rgba = image::DynamicImage::ImageRgb8(rgb).to_rgba8();

    match wm.position.as_str() {
        "tiled" => apply_tiled_watermark(font, &mut rgba, wm, scale, color),
        "diagonal" => apply_diagonal_watermark(font, &mut rgba, wm, scale, color),
        pos => {
            let text_w = measure_text_width(font, &wm.text, scale);
            let text_h = {
                let s = font.as_scaled(scale);
                (s.ascent() - s.descent()).ceil() as u32
            };
            let margin = (rgba.width() as f32 * 0.01) as u32;
            let (x, y) =
                calc_watermark_position(pos, rgba.width(), rgba.height(), text_w, text_h, margin);
            draw_text_on(
                font, &mut rgba, color, x as i32, y as i32, scale, &wm.text, wm.bold,
            );
        }
    }

    Ok(image::DynamicImage::ImageRgba8(rgba).into_rgb8())
}

#[tauri::command]
async fn open_folder(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("無法開啟: {}", e))?;

    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("無法開啟: {}", e))?;

    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("無法開啟: {}", e))?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(ProcessingState {
            cancel_flag: Arc::new(AtomicBool::new(false)),
            output_files: Arc::new(Mutex::new(Vec::new())),
        })
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            shrink_image,
            compress_image,
            cancel_processing,
            check_output_dir_exists,
            watermark_only,
            rename_images,
            open_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
