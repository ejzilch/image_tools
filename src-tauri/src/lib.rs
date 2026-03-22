use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use chrono::Local;
use fast_image_resize::{self as fir, Resizer};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

static FONT_DATA: &[u8] = include_bytes!("../fonts/NotoSansTC-VariableFont_wght.ttf");

#[derive(serde::Serialize, Clone)]
struct ProgressPayload {
    current: usize,
    total: usize,
    file: String,
    success: bool,
    error: Option<String>,
}

pub struct ProcessingState {
    pub cancel_flag: Arc<AtomicBool>,
    pub output_files: Arc<Mutex<Vec<PathBuf>>>,
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
    let supported = ["jpg", "jpeg", "png", "webp", "tiff", "tif"];
    if path.is_dir() {
        fs::read_dir(path)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| supported.contains(&e.to_lowercase().as_str()))
                    .unwrap_or(false)
            })
            .collect()
    } else {
        vec![path.to_path_buf()]
    }
}

// mozjpeg decode，只處理 JPEG，其他格式回傳 None
fn decode_jpeg_mozjpeg(bytes: &[u8]) -> Result<image::DynamicImage, String> {
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

    image::RgbImage::from_raw(width, height, pixels)
        .map(image::DynamicImage::ImageRgb8)
        .ok_or("建立圖片失敗".to_string())
}

// 根據副檔名選擇 decode 方式
fn decode_image(bytes: &[u8], ext: &str) -> Result<image::DynamicImage, String> {
    if ext == "jpg" || ext == "jpeg" {
        decode_jpeg_mozjpeg(bytes)
    } else {
        image::load_from_memory(bytes).map_err(|e| format!("無法開啟: {}", e))
    }
}

fn resize_with_fir(
    img: &image::DynamicImage,
    dst_width: u32,
    dst_height: u32,
) -> Result<image::DynamicImage, String> {
    let src = fir::images::Image::from_vec_u8(
        img.width(),
        img.height(),
        img.to_rgb8().into_raw(),
        fir::PixelType::U8x3,
    )
    .map_err(|e| format!("fir 建立失敗: {}", e))?;

    let mut dst = fir::images::Image::new(dst_width, dst_height, fir::PixelType::U8x3);

    let mut resizer = Resizer::new();
    resizer
        .resize(
            &src,
            &mut dst,
            &fir::ResizeOptions::new()
                .resize_alg(fir::ResizeAlg::Convolution(fir::FilterType::Lanczos3)),
        )
        .map_err(|e| format!("resize 失敗: {}", e))?;

    Ok(image::DynamicImage::ImageRgb8(
        image::RgbImage::from_raw(dst_width, dst_height, dst.into_vec()).ok_or("轉換失敗")?,
    ))
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

    let raw = rgb.as_raw();
    let row_stride = width as usize * 3;
    for row in raw.chunks(row_stride) {
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
    // 目標大小的 10%，容忍誤差最少 50KB 最多 200KB
    let tolerance = (target_bytes / 10).clamp(50 * 1024, 200 * 1024);
    let mut low: u8 = 1;
    let mut high: u8 = 95;
    let mut best: Vec<u8> = Vec::new();

    while low <= high {
        let mid = low + (high - low) / 2;
        let bytes = encode_jpeg_mozjpeg(img, mid)?;
        let size = bytes.len() as u64;

        // ✅ 在 target ± 100KB 範圍內直接返回
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
    // 每次開始前重置
    state.cancel_flag.store(false, Ordering::Relaxed);
    state.output_files.lock().unwrap().clear();

    // spawn_blocking 之前先把 Arc clone 出來
    let cancel_flag = state.cancel_flag.clone();
    let output_files = state.output_files.clone();

    tokio::task::spawn_blocking(move || {
        let images: Vec<PathBuf> = inputs
            .iter()
            .flat_map(|input| collect_images(input))
            .collect();

        if images.is_empty() {
            return Ok(vec![]);
        }

        let total = images.len();
        app.emit("progress_total", total).ok();
        let counter = std::sync::atomic::AtomicUsize::new(0);

        // 預先建立所有輸出資料夾
        let _out_dirs: HashSet<PathBuf> = images
            .iter()
            .map(|p| ensure_output_dir(p, "shrink"))
            .collect();

        let outputs = Mutex::new(Vec::new());
        let errors = Mutex::new(Vec::new());

        images.par_iter().for_each(|img_path| {
            // 檢查取消旗標
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }
            let result = (|| -> Result<String, String> {
                let bytes =
                    fs::read(img_path).map_err(|e| format!("讀取失敗 {:?}: {}", img_path, e))?;

                let ext = img_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                let img = decode_image(&bytes, &ext)?;

                let (dst_w, dst_h) = if shrink_mode == "dimension" {
                    (width, height)
                } else {
                    (
                        (img.width() as f32 * ratio / 100.0) as u32,
                        (img.height() as f32 * ratio / 100.0) as u32,
                    )
                };

                let resized = resize_with_fir(&img, dst_w, dst_h)?;

                let final_img = if let Some(ref wm) = watermark {
                    apply_watermark(resized, wm)?
                } else {
                    resized
                };

                let out_dir = ensure_output_dir(img_path, "shrink");
                let out = output_path_with_dir(&out_dir, img_path);

                save_image(&final_img, &out, &ext)?;

                Ok(out.to_string_lossy().to_string())
            })();

            let current = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let file_name = img_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            match result {
                Ok(path) => {
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
                    output_files.lock().unwrap().push(PathBuf::from(&path));
                    outputs.lock().unwrap().push(path);
                }
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
                    errors.lock().unwrap().push(e);
                }
            }
        });

        let errors = errors.into_inner().unwrap();
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        Ok(outputs.into_inner().unwrap())
    })
    .await
    .map_err(|e| format!("執行緒錯誤: {}", e))?
}

#[tauri::command]
async fn cancel_processing(state: tauri::State<'_, ProcessingState>) -> Result<(), String> {
    // 設定取消旗標
    state.cancel_flag.store(true, Ordering::Relaxed);

    // 刪除這批已輸出的檔案
    let files = state.output_files.lock().unwrap().clone();
    for path in files {
        fs::remove_file(&path).ok();
    }
    state.output_files.lock().unwrap().clear();

    Ok(())
}

#[tauri::command]
async fn compress_image(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProcessingState>,
    inputs: Vec<String>,
    target_bytes: u64,
    watermark: Option<WatermarkOptions>,
) -> Result<Vec<String>, String> {
    // 每次開始前重置
    state.cancel_flag.store(false, Ordering::Relaxed);
    state.output_files.lock().unwrap().clear();

    // spawn_blocking 之前先把 Arc clone 出來
    let cancel_flag = state.cancel_flag.clone();
    let output_files = state.output_files.clone();

    tokio::task::spawn_blocking(move || {
        let images: Vec<PathBuf> = inputs
            .iter()
            .flat_map(|input| collect_images(input))
            .collect();

        if images.is_empty() {
            return Ok(vec![]);
        }

        let total = images.len();
        app.emit("progress_total", total).ok();
        let counter = std::sync::atomic::AtomicUsize::new(0);

        let outputs = Mutex::new(Vec::new());
        let errors = Mutex::new(Vec::new());

        images.par_iter().for_each(|img_path| {
            // 檢查取消旗標
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }
            let result = (|| -> Result<String, String> {
                let bytes =
                    fs::read(img_path).map_err(|e| format!("讀取失敗 {:?}: {}", img_path, e))?;

                let out_dir = ensure_output_dir(img_path, "compress");
                let out = output_path_with_dir(&out_dir, img_path);

                // 原本就符合大小，直接複製
                if (bytes.len() as u64) <= target_bytes {
                    // 如果有浮水印，還是要 decode 後加浮水印再存
                    if let Some(ref wm) = watermark {
                        let ext = img_path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("jpg")
                            .to_lowercase();
                        let img = decode_image(&bytes, &ext)?;
                        let final_img = apply_watermark(img, wm)?;

                        save_image(&final_img, &out, &ext)?;
                    } else {
                        fs::write(&out, &bytes).map_err(|e| format!("複製失敗: {}", e))?;
                    }
                    return Ok(out.to_string_lossy().to_string());
                }

                let ext = img_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("jpg")
                    .to_lowercase();

                let img = decode_image(&bytes, &ext)?;

                let final_img = if let Some(ref wm) = watermark {
                    apply_watermark(img, wm)?
                } else {
                    img
                };

                if ext == "jpg" || ext == "jpeg" {
                    let compressed = compress_to_target(&final_img, target_bytes)?;
                    fs::write(&out, compressed).map_err(|e| format!("無法儲存: {}", e))?;
                } else {
                    // 其它轉 JPG
                    let out_jpg = out.with_extension("jpg");
                    let compressed = compress_to_target(&final_img, target_bytes)?;
                    fs::write(&out_jpg, compressed).map_err(|e| format!("無法儲存: {}", e))?;
                }

                Ok(out.to_string_lossy().to_string())
            })();

            let current = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let file_name = img_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            match result {
                Ok(path) => {
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

                    output_files.lock().unwrap().push(PathBuf::from(&path));
                    outputs.lock().unwrap().push(path);
                }
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
                    errors.lock().unwrap().push(e);
                }
            }
        });

        let errors = errors.into_inner().unwrap();
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        Ok(outputs.into_inner().unwrap())
    })
    .await
    .map_err(|e| format!("執行緒錯誤: {}", e))?
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

#[tauri::command]
async fn watermark_only(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProcessingState>,
    inputs: Vec<String>,
    watermark: WatermarkOptions,
) -> Result<Vec<String>, String> {
    state.cancel_flag.store(false, Ordering::Relaxed);
    state.output_files.lock().unwrap().clear();

    let cancel_flag = state.cancel_flag.clone();
    let output_files_arc = state.output_files.clone();

    tokio::task::spawn_blocking(move || {
        let images: Vec<PathBuf> = inputs
            .iter()
            .flat_map(|input| collect_images(input))
            .collect();

        if images.is_empty() {
            return Ok(vec![]);
        }

        let total = images.len();
        app.emit("progress_total", total).ok();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let outputs = Mutex::new(Vec::new());
        let errors = Mutex::new(Vec::new());

        images.par_iter().for_each(|img_path| {
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }

            let result = (|| -> Result<String, String> {
                let bytes = fs::read(img_path).map_err(|e| format!("讀取失敗: {}", e))?;
                let ext = img_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                let img = decode_image(&bytes, &ext)?;
                let final_img = apply_watermark(img, &watermark)?;

                let out_dir = ensure_output_dir(img_path, "watermark");
                let out = output_path_with_dir(&out_dir, img_path);

                save_image(&final_img, &out, &ext)?;

                Ok(out.to_string_lossy().to_string())
            })();

            let current = counter.fetch_add(1, Ordering::Relaxed) + 1;
            let file_name = img_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            match result {
                Ok(path) => {
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
                    output_files_arc.lock().unwrap().push(PathBuf::from(&path));
                    outputs.lock().unwrap().push(path);
                }
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
                    errors.lock().unwrap().push(e);
                }
            }
        });

        let errors = errors.into_inner().unwrap();
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        Ok(outputs.into_inner().unwrap())
    })
    .await
    .map_err(|e| format!("執行緒錯誤: {}", e))?
}

fn measure_text_width(font: &FontVec, text: &str, scale: PxScale) -> u32 {
    use ab_glyph::{Font, ScaleFont};
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
        "bottom-right" => (
            img_w.saturating_sub(text_w + margin),
            img_h.saturating_sub(text_h + margin),
        ),
        _ => (
            img_w.saturating_sub(text_w + margin),
            img_h.saturating_sub(text_h + margin),
        ),
    }
}

fn apply_tiled_watermark(
    img: &mut image::RgbaImage,
    text: &str,
    font: &ab_glyph::FontVec,
    scale: PxScale,
    color: image::Rgba<u8>,
    bold: bool,
) {
    let text_w = measure_text_width(font, text, scale);
    let scaled = font.as_scaled(scale);
    let text_h = (scaled.ascent() - scaled.descent()).ceil() as u32;

    let img_w = img.width();
    let img_h = img.height();

    let min_rows = 5u32;
    let spacing_y = (img_h / min_rows).max(text_h * 3);
    let spacing_x = text_w * 3;
    let offset = spacing_x / 2;

    let mut row = 0i32;
    let mut y = 0i32;
    while y < img_h as i32 + text_h as i32 {
        let x_offset = if row % 2 == 0 { 0 } else { offset as i32 };
        let mut x = -(text_w as i32) + x_offset;
        while x < img_w as i32 + text_w as i32 {
            if bold {
                draw_text_bold(img, color, x, y, scale, font, text);
            } else {
                imageproc::drawing::draw_text_mut(img, color, x, y, scale, font, text);
            }
            x += spacing_x as i32;
        }
        y += spacing_y as i32;
        row += 1;
    }
}

fn apply_diagonal_watermark(
    img: &mut image::RgbaImage,
    text: &str,
    font: &ab_glyph::FontVec,
    scale: PxScale,
    color: image::Rgba<u8>,
    bold: bool,
) {
    let img_w = img.width();
    let img_h = img.height();

    let text_w = measure_text_width(font, text, scale);
    let scaled_font = font.as_scaled(scale);
    let text_h = (scaled_font.ascent() - scaled_font.descent()).ceil() as u32;

    // 計算角度（左下到右上）
    let angle_rad = (img_h as f32 / img_w as f32).atan();

    // 先把文字畫在小圖層上
    let padding = 10u32;
    let diagonal = ((text_w * text_w + text_h * text_h) as f32).sqrt() as u32;
    let canvas_size = diagonal + padding * 4;
    let mut text_layer = image::RgbaImage::new(canvas_size, canvas_size);

    // 文字畫在畫布中央
    let tx = (canvas_size / 2).saturating_sub(text_w / 2) as i32;
    let ty = (canvas_size / 2).saturating_sub(text_h / 2) as i32;
    if bold {
        draw_text_bold(&mut text_layer, color, tx, ty, scale, font, text);
    } else {
        imageproc::drawing::draw_text_mut(&mut text_layer, color, tx, ty, scale, font, text);
    }

    // 旋轉文字圖層
    let rotated = imageproc::geometric_transformations::rotate_about_center(
        &text_layer,
        -angle_rad,
        imageproc::geometric_transformations::Interpolation::Bilinear,
        image::Rgba([0, 0, 0, 0]),
    );

    let rw = rotated.width() as i64;
    let rh = rotated.height() as i64;

    // 間距設定
    let spacing_x = (text_w as f32 * 2.6) as i64; // 水平間距
    let min_rows = 5u32;
    let spacing_y = (img_h as i64 / min_rows as i64).max(text_h as i64 * 3);
    let offset = spacing_x / 2; // 每列錯開半個間距

    // 滿版貼上，每列錯開
    let mut row = 0i64;
    let mut y = -(rh);
    while y < img_h as i64 + rh {
        let x_offset = if row % 2 == 0 { 0 } else { offset }; // 奇偶列錯開
        let mut x = -(rw) + x_offset;
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
    let font = ab_glyph::FontVec::try_from_vec(FONT_DATA.to_vec())
        .map_err(|e| format!("字型載入失敗: {}", e))?;

    let scale_factor = img.width() as f32 / 1000.0;
    let scaled_size = wm.font_size * scale_factor;
    let scale = PxScale::from(scaled_size);
    let alpha = (wm.opacity * 255.0) as u8;
    let text_color = image::Rgba([wm.color[0], wm.color[1], wm.color[2], alpha]);

    let mut img_rgba = img.to_rgba8();

    if wm.position == "tiled" {
        apply_tiled_watermark(&mut img_rgba, &wm.text, &font, scale, text_color, wm.bold);
    } else if wm.position == "diagonal" {
        apply_diagonal_watermark(&mut img_rgba, &wm.text, &font, scale, text_color, wm.bold);
    } else {
        let scaled = font.as_scaled(scale);
        let text_h = (scaled.ascent() - scaled.descent()).ceil() as u32;
        let text_w = measure_text_width(&font, &wm.text, scale);
        let margin = (img_rgba.width() as f32 * 0.01) as u32;
        let (x, y) = calc_watermark_position(
            &wm.position,
            img_rgba.width(),
            img_rgba.height(),
            text_w,
            text_h,
            margin,
        );
        if wm.bold {
            draw_text_bold(
                &mut img_rgba,
                text_color,
                x as i32,
                y as i32,
                scale,
                &font,
                &wm.text,
            );
        } else {
            imageproc::drawing::draw_text_mut(
                &mut img_rgba,
                text_color,
                x as i32,
                y as i32,
                scale,
                &font,
                &wm.text,
            );
        }
    }

    Ok(image::DynamicImage::ImageRgba8(img_rgba))
}

fn draw_text_bold(
    img: &mut image::RgbaImage,
    color: image::Rgba<u8>,
    x: i32,
    y: i32,
    scale: PxScale,
    font: &ab_glyph::FontVec,
    text: &str,
) {
    for dx in -1i32..=1 {
        for dy in -1i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            imageproc::drawing::draw_text_mut(img, color, x + dx, y + dy, scale, font, text);
        }
    }
    imageproc::drawing::draw_text_mut(img, color, x, y, scale, font, text);
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
    state.output_files.lock().unwrap().clear();

    let cancel_flag = state.cancel_flag.clone();
    let output_files = state.output_files.clone();

    tokio::task::spawn_blocking(move || {
        let images: Vec<PathBuf> = inputs
            .iter()
            .flat_map(|input| collect_images(input))
            .collect();

        if images.is_empty() {
            return Ok(vec![]);
        }

        let total = images.len();
        app.emit("progress_total", total).ok();

        let counter = std::sync::atomic::AtomicUsize::new(1);
        let date_str = Local::now().format("%Y-%m-%d").to_string();

        let outputs = Mutex::new(Vec::new());
        let errors = Mutex::new(Vec::new());

        images.par_iter().for_each(|img_path| {
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }

            let result = (|| -> Result<String, String> {
                let num = counter.fetch_add(1, Ordering::Relaxed);
                if num > 99999 {
                    return Err("流水號超過上限 99999".to_string());
                }

                let seq = format!("{:05}", num);
                let ext = img_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("jpg")
                    .to_lowercase();

                let new_stem = match rename_mode.as_str() {
                    "date_sequence" => format!("{}_{}_{}", custom_text, date_str, seq),
                    _ => format!("{}_{}", custom_text, seq),
                };

                let new_name = format!("{}.{}", new_stem, ext);
                let out_dir = ensure_output_dir(img_path, "rename");
                let out = out_dir.join(&new_name);

                if let Some(ref wm) = watermark {
                    let bytes = fs::read(img_path).map_err(|e| format!("讀取失敗: {}", e))?;
                    let img = decode_image(&bytes, &ext)?;
                    let final_img = apply_watermark(img, wm)?;

                    save_image(&final_img, &out, &ext)?;
                } else {
                    // 無浮水印：直接複製，速度最快
                    fs::copy(img_path, &out).map_err(|e| format!("複製失敗: {}", e))?;
                }

                Ok(out.to_string_lossy().to_string())
            })();

            let current = counter.load(Ordering::Relaxed) - 1;
            let file_name = img_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            match result {
                Ok(path) => {
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
                    output_files.lock().unwrap().push(PathBuf::from(&path));
                    outputs.lock().unwrap().push(path);
                }
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
                    errors.lock().unwrap().push(e);
                }
            }
        });

        let errors = errors.into_inner().unwrap();
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        Ok(outputs.into_inner().unwrap())
    })
    .await
    .map_err(|e| format!("執行緒錯誤: {}", e))?
}

fn save_image(img: &image::DynamicImage, out: &Path, ext: &str) -> Result<(), String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let fmt = image::ImageFormat::from_extension(ext).unwrap_or(image::ImageFormat::Jpeg);
    img.write_to(&mut buf, fmt)
        .map_err(|e| format!("編碼失敗: {}", e))?;
    fs::write(out, buf.into_inner()).map_err(|e| format!("無法儲存: {}", e))?;
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
