use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use chrono::Local;
use fast_image_resize::{self as fir, Resizer};
use once_cell::sync::Lazy;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

// Global statics
static FONT_DATA: &[u8] = include_bytes!("../fonts/NotoSansTC-VariableFont_wght.ttf");

static FONT: Lazy<FontVec> =
    Lazy::new(|| FontVec::try_from_vec(FONT_DATA.to_vec()).expect("字型載入失敗"));

static SUPPORTED_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "tiff", "tif"];

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
    pub position: String,
    pub opacity: f32,
    pub color: [u8; 3],
    pub font_size: f32,
    pub bold: bool,
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
        // watermark 需要 RGBA，所已完成後轉回 RGB 維持原則一致性
        let dyn_img = image::DynamicImage::ImageRgb8(rgb);
        let result = apply_watermark(dyn_img, &self.options)?;
        Ok(result.into_rgb8())
    }
}

struct ProcessOptions {
    ops: Vec<Box<dyn ImageOp>>,
    compress_target: Option<u64>,
    rename_stem: Option<String>,
    out_suffix: String,
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

fn load_image(img_path: &Path) -> Result<LoadedImage, String> {
    let raw_bytes = fs::read(img_path).map_err(|e| format!("讀取失敗 {:?}: {}", img_path, e))?;
    let ext = img_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let rgb = decode_to_rgb(&raw_bytes, &ext)?;
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
    rgb: &image::RgbImage,
    raw_bytes: &[u8],
    ext: &str,
    out: &Path,
    compress_target: Option<u64>,
    has_ops: bool,
) -> Result<PathBuf, String> {
    if let Some(target) = compress_target {
        // 原本就夠小且無任何 output 直接複製
        if (raw_bytes.len() as u64) <= target && !has_ops {
            fs::write(out, raw_bytes).map_err(|e| format!("複製失敗: {}", e))?;
            return Ok(out.to_path_buf());
        }
        let dyn_img = image::DynamicImage::ImageRgb8(rgb.clone());
        if matches!(ext, "jpg" | "jpeg") {
            let compressed = compress_to_target(&dyn_img, target)?;
            fs::write(out, compressed).map_err(|e| format!("無法儲存: {}", e))?;
            Ok(out.to_path_buf())
        } else {
            let out_jpg = out.with_extension("jpg");
            let compressed = compress_to_target(&dyn_img, target)?;
            fs::write(&out_jpg, compressed).map_err(|e| format!("無法儲存: {}", e))?;
            Ok(out_jpg)
        }
    } else {
        let dyn_img = image::DynamicImage::ImageRgb8(rgb.clone());
        write_image(&dyn_img, out, ext)?;
        Ok(out.to_path_buf())
    }
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
        &rgb,
        &loaded.raw_bytes,
        &loaded.ext,
        &out,
        opts.compress_target,
        has_ops,
    )?;

    if cancel_flag.load(Ordering::Relaxed) {
        fs::remove_file(&out_path).ok();
        return Err("cancelled".into());
    }

    Ok(out_path)
}

struct BatchResult {
    outputs: Vec<String>,
    errors: Vec<String>,
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
            let result = process_one(img_path, &opts, &cancel_flag);

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
    let mut errors: Vec<String> = Vec::new();

    for (_, _, result) in results {
        match result {
            Ok(path) => {
                success_paths.push(path.clone());
                outputs.push(path.to_string_lossy().to_string());
            }
            Err(e) if e == "cancelled" => { /* 取消就跳過 */ }
            Err(e) => {
                errors.push(e);
            }
        }
    }

    // collect 完成後，檢查是否已取消
    if cancel_flag.load(Ordering::Relaxed) {
        for path in &success_paths {
            fs::remove_file(path).ok();
        }
        return BatchResult {
            outputs: vec![],
            errors: vec![],
        };
    }

    *output_files.lock().expect("poisoned mutex") = success_paths;

    BatchResult { outputs, errors }
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
) -> Result<Vec<String>, String> {
    state.cancel_flag.store(false, Ordering::Relaxed);
    state.output_files.lock().expect("poisoned mutex").clear();

    let cancel_flag = state.cancel_flag.clone();
    let output_files = state.output_files.clone();

    tokio::task::spawn_blocking(move || {
        let images = collect_images_from_inputs(&inputs);
        if images.is_empty() {
            return Ok(vec![]);
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
                }
            },
        );

        if !batch.errors.is_empty() {
            return Err(batch.errors.join("\n"));
        }
        Ok(batch.outputs)
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
) -> Result<Vec<String>, String> {
    state.cancel_flag.store(false, Ordering::Relaxed);
    state.output_files.lock().expect("poisoned mutex").clear();

    let cancel_flag = state.cancel_flag.clone();
    let output_files = state.output_files.clone();

    tokio::task::spawn_blocking(move || {
        let images = collect_images_from_inputs(&inputs);
        if images.is_empty() {
            return Ok(vec![]);
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
                }
            },
        );

        if !batch.errors.is_empty() {
            return Err(batch.errors.join("\n"));
        }
        Ok(batch.outputs)
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
) -> Result<Vec<String>, String> {
    state.cancel_flag.store(false, Ordering::Relaxed);
    state
        .output_files
        .lock()
        .expect("poisoned mutex from watermark_only.")
        .clear();

    let cancel_flag = state.cancel_flag.clone();
    let output_files = state.output_files.clone();

    tokio::task::spawn_blocking(move || {
        let images = collect_images_from_inputs(&inputs);
        if images.is_empty() {
            return Ok(vec![]);
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
            },
        );

        if !batch.errors.is_empty() {
            return Err(batch.errors.join("\n"));
        }
        Ok(batch.outputs)
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
) -> Result<Vec<String>, String> {
    state.cancel_flag.store(false, Ordering::Relaxed);
    state
        .output_files
        .lock()
        .expect("poisoned mutex from rename_images.")
        .clear();

    let cancel_flag = state.cancel_flag.clone();
    let output_files = state.output_files.clone();

    tokio::task::spawn_blocking(move || {
        let images = collect_images_from_inputs(&inputs);
        if images.is_empty() {
            return Ok(vec![]);
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
                }
            },
        );

        if !batch.errors.is_empty() {
            return Err(batch.errors.join("\n"));
        }
        Ok(batch.outputs)
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

fn encode_jpeg_mozjpeg(img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let rgb = img.to_rgb8();
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

fn compress_to_target(img: &image::DynamicImage, target_bytes: u64) -> Result<Vec<u8>, String> {
    let tolerance = (target_bytes / 10).clamp(50 * 1024, 200 * 1024);
    let mut low: u8 = 1;
    let mut high: u8 = 95;
    let mut best: Vec<u8> = Vec::new();

    while low <= high {
        let mid = low + (high - low) / 2;
        let bytes = encode_jpeg_mozjpeg(img, mid)?;
        let size = bytes.len() as u64;

        if size.abs_diff(target_bytes) <= tolerance {
            return Ok(bytes);
        }

        if size <= target_bytes {
            best = bytes;
            low = mid + 1;
        } else {
            if mid == 0 {
                break;
            }
            high = mid - 1;
        }
    }

    if best.is_empty() {
        best = encode_jpeg_mozjpeg(img, 1)?;
    }
    Ok(best)
}

fn write_image(img: &image::DynamicImage, out: &Path, ext: &str) -> Result<(), String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let fmt = image::ImageFormat::from_extension(ext).unwrap_or(image::ImageFormat::Jpeg);
    img.write_to(&mut buf, fmt)
        .map_err(|e| format!("編碼失敗: {}", e))?;
    fs::write(out, buf.into_inner()).map_err(|e| format!("無法儲存: {}", e))?;
    Ok(())
}

fn measure_text_width(text: &str, scale: PxScale) -> u32 {
    let font = &*FONT;
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
    img: &mut image::RgbaImage,
    color: image::Rgba<u8>,
    x: i32,
    y: i32,
    scale: PxScale,
    text: &str,
    bold: bool,
) {
    let font = &*FONT;
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
    img: &mut image::RgbaImage,
    wm: &WatermarkOptions,
    scale: PxScale,
    color: image::Rgba<u8>,
) {
    let font = &*FONT;
    let text_w = measure_text_width(&wm.text, scale);
    let text_h = {
        let s = font.as_scaled(scale);
        (s.ascent() - s.descent()).ceil() as u32
    };
    let (img_w, img_h) = (img.width(), img.height());
    let spacing_y = (img_h / 5).max(text_h * 3);
    let spacing_x = text_w * 3;
    let offset = spacing_x / 2;

    let mut row = 0i32;
    let mut y = 0i32;
    while y < img_h as i32 + text_h as i32 {
        let x_off = if row % 2 == 0 { 0 } else { offset as i32 };
        let mut x = -(text_w as i32) + x_off;
        while x < img_w as i32 + text_w as i32 {
            draw_text_on(img, color, x, y, scale, &wm.text, wm.bold);
            x += spacing_x as i32;
        }
        y += spacing_y as i32;
        row += 1;
    }
}

fn apply_diagonal_watermark(
    img: &mut image::RgbaImage,
    wm: &WatermarkOptions,
    scale: PxScale,
    color: image::Rgba<u8>,
) {
    let font = &*FONT;
    let (img_w, img_h) = (img.width(), img.height());
    let text_w = measure_text_width(&wm.text, scale);
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
    draw_text_on(&mut text_layer, color, tx, ty, scale, &wm.text, wm.bold);

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

fn apply_watermark(
    img: image::DynamicImage,
    wm: &WatermarkOptions,
) -> Result<image::DynamicImage, String> {
    let font = &*FONT;
    let scale = PxScale::from(wm.font_size * (img.width() as f32 / 1000.0));
    let color = image::Rgba([
        wm.color[0],
        wm.color[1],
        wm.color[2],
        (wm.opacity * 255.0) as u8,
    ]);
    let mut rgba = img.to_rgba8();

    match wm.position.as_str() {
        "tiled" => apply_tiled_watermark(&mut rgba, wm, scale, color),
        "diagonal" => apply_diagonal_watermark(&mut rgba, wm, scale, color),
        pos => {
            let text_w = measure_text_width(&wm.text, scale);
            let text_h = {
                let s = font.as_scaled(scale);
                (s.ascent() - s.descent()).ceil() as u32
            };
            let margin = (rgba.width() as f32 * 0.01) as u32;
            let (x, y) =
                calc_watermark_position(pos, rgba.width(), rgba.height(), text_w, text_h, margin);
            draw_text_on(
                &mut rgba, color, x as i32, y as i32, scale, &wm.text, wm.bold,
            );
        }
    }

    Ok(image::DynamicImage::ImageRgba8(rgba))
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
