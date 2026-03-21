use fast_image_resize::{self as fir, Resizer};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

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

                let out_dir = ensure_output_dir(img_path, "shrink");
                let out = output_path_with_dir(&out_dir, img_path);
                resized
                    .save(&out)
                    .map_err(|e| format!("無法儲存 {:?}: {}", out, e))?;

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
                    fs::write(&out, &bytes).map_err(|e| format!("複製失敗: {}", e))?;
                    return Ok(out.to_string_lossy().to_string());
                }

                let ext = img_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("jpg")
                    .to_lowercase();

                let img = decode_image(&bytes, &ext)?;

                if ext == "jpg" || ext == "jpeg" {
                    let compressed = compress_to_target(&img, target_bytes)?;
                    fs::write(&out, compressed).map_err(|e| format!("無法儲存: {}", e))?;
                } else {
                    // 其它轉 JPG
                    let out_jpg = out.with_extension("jpg");
                    let compressed = compress_to_target(&img, target_bytes)?;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(ProcessingState {
            cancel_flag: Arc::new(AtomicBool::new(false)),
            output_files: Arc::new(Mutex::new(Vec::new())),
        })
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            shrink_image,
            compress_image,
            cancel_processing,
            check_output_dir_exists,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
