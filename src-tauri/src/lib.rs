use fast_image_resize::{self as fir, Resizer};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Emitter;

#[derive(serde::Serialize, Clone)]
struct ProgressPayload {
    current: usize,
    total: usize,
    file: String,
    success: bool,
    error: Option<String>,
}

fn ensure_output_dir(input: &Path, suffix: &str) -> PathBuf {
    let parent = input.parent().unwrap_or(Path::new("."));
    let folder_name = parent
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    let out_dir = parent
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{}_{}", folder_name, suffix));
    fs::create_dir_all(&out_dir).ok();
    out_dir
}

fn output_path_with_dir(out_dir: &Path, input: &Path, suffix: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
    out_dir.join(format!("{}_{}.{}", stem, suffix, ext))
}

fn collect_images(input: &str) -> Vec<PathBuf> {
    let path = Path::new(input);
    let supported = ["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif"];
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
    // 1. 用小圖取樣壓縮率（速度快）
    let sample_size = 256u32;
    let sample = img.resize_exact(
        sample_size,
        sample_size,
        image::imageops::FilterType::Nearest,
    );

    // 2. quality=80 encode 小圖，算出每像素佔幾 bytes
    let sample_bytes = encode_jpeg_mozjpeg(&sample, 80)?;
    let bytes_per_pixel_at_80 = sample_bytes.len() as f64 / (sample_size * sample_size) as f64;

    // 3. 估算原圖在 quality=80 時的大小
    let original_pixels = (img.width() * img.height()) as f64;
    let estimated_size_at_80 = bytes_per_pixel_at_80 * original_pixels;

    // 4. 用經驗公式估算需要的 quality（JPEG 大小 ∝ quality^1.5）
    let estimated_quality = {
        let ratio = target_bytes as f64 / estimated_size_at_80;
        let q = 80.0 * ratio.powf(1.0 / 1.5);
        (q as u8).clamp(1, 95)
    };

    // 5. 直接 encode 原圖
    let result = encode_jpeg_mozjpeg(img, estimated_quality)?;

    if result.len() as u64 <= target_bytes {
        // 符合目標，試看看能不能再高一點保持畫質
        let higher_q = (estimated_quality + 5).min(95);
        let better = encode_jpeg_mozjpeg(img, higher_q)?;
        if better.len() as u64 <= target_bytes {
            return Ok(better);
        }
        return Ok(result);
    } else {
        // 稍微超過，往下修正一次
        let lower_q = estimated_quality.saturating_sub(5).max(1);
        Ok(encode_jpeg_mozjpeg(img, lower_q)?)
    }
}

#[tauri::command]
async fn shrink_image(
    app: tauri::AppHandle,
    inputs: Vec<String>,
    shrink_mode: String,
    width: u32,
    height: u32,
    ratio: f32,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let images: Vec<PathBuf> = inputs
            .iter()
            .flat_map(|input| collect_images(input))
            .collect();

        if images.is_empty() {
            return Ok(vec![]);
        }

        let total = images.len();
        let counter = std::sync::atomic::AtomicUsize::new(0);

        // 預先建立所有輸出資料夾
        let _out_dirs: HashSet<PathBuf> = images
            .iter()
            .map(|p| ensure_output_dir(p, "shrink"))
            .collect();

        let outputs = Mutex::new(Vec::new());
        let errors = Mutex::new(Vec::new());

        images.par_iter().for_each(|img_path| {
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
                let out = output_path_with_dir(&out_dir, img_path, "shrink");
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
async fn compress_image(
    app: tauri::AppHandle,
    inputs: Vec<String>,
    target_bytes: u64,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let images: Vec<PathBuf> = inputs
            .iter()
            .flat_map(|input| collect_images(input))
            .collect();

        if images.is_empty() {
            return Ok(vec![]);
        }

        let total = images.len();
        let counter = std::sync::atomic::AtomicUsize::new(0);

        let outputs = Mutex::new(Vec::new());
        let errors = Mutex::new(Vec::new());

        images.par_iter().for_each(|img_path| {
            let result = (|| -> Result<String, String> {
                let bytes =
                    fs::read(img_path).map_err(|e| format!("讀取失敗 {:?}: {}", img_path, e))?;

                let out_dir = ensure_output_dir(img_path, "compress");
                let out = output_path_with_dir(&out_dir, img_path, "compress");

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
                    img.save(&out).map_err(|e| format!("無法儲存: {}", e))?;
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
fn check_output_dir_exists(input: String, suffix: String) -> bool {
    let path = Path::new(&input);
    let parent = path.parent().unwrap_or(Path::new("."));
    let folder_name = parent
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    let out_dir = parent
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{}_{}", folder_name, suffix));
    out_dir.exists()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            shrink_image,
            compress_image,
            check_output_dir_exists,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
