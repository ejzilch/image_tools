# Image Tools

基於 [Tauri](https://tauri.app/) + Rust 開發的桌面圖片批次處理工具，支援圖片縮圖與壓縮功能。

---

## 目前功能

### 縮圖
- 指定寬高（px）
- 維持比例縮小（%）
- 使用 SIMD 加速的 `fast_image_resize`，速度快

### 壓縮
- 壓縮至指定大小（KB / MB）
- 採用 Binary search 和 &plusmn; 誤差值來提高效率
- JPEG 使用 `mozjpeg` 加速 decode / encode

### 浮水印支援
- 縮圖 + 輸出浮水印
- 壓縮 + 輸出浮水印
- 純輸出浮水印


### 通用
- 支援批次處理（多檔案 / 整個資料夾）
- 平行處理（`rayon`），多核心同時處理
- 即時進度條顯示
- 輸出結果自動建立對應資料夾（`原資料夾_shrink` / `原資料夾_compress`）

### 支援格式
`jpg` / `jpeg` / `png` / `gif` / `bmp` / `webp` / `tiff` / `tif`

> [!WARNING]
> 支援格式會將無法壓縮的圖檔轉換為 `jpg` 格式進行壓縮且輸出

---

## 開發環境需求

| 工具 | 版本需求 |
|------|----------|
| [Rust](https://www.rust-lang.org/) | 1.77.2 以上 |
| [Node.js](https://nodejs.org/) | 18 以上 |
| [Tauri CLI](https://tauri.app/start/) | v2 |
| [NASM](https://www.nasm.us/) | 任意版本（Windows 需要，用於編譯 mozjpeg SIMD） |

### NASM 安裝（Windows）

1. 至 [nasm.us](https://www.nasm.us/pub/nasm/releasebuilds/) 下載最新版 installer
2. 安裝時勾選 **Add to PATH**
3. 重開終端機確認：

```powershell
nasm -v
```

---

## 執行方式

### 安裝依賴

```bash
npm install
```

### 開發模式

```bash
cargo tauri dev
```

### 正式 Build

```bash
cargo tauri build
```

Build 產出的執行檔位於 `src-tauri/target/release/`，使用者**不需要**安裝 Rust、NASM 或任何依賴，直接執行即可。

---

## 專案結構

```
image_tools/
├── src/                  # 前端 (Svelte)
│   └── App.svelte
├── src-tauri/            # 後端 (Rust)
│   ├── src/
│   │   └── lib.rs        # 核心邏輯
│   └── Cargo.toml
└── package.json
```

---

## 主要依賴

| 套件 | 用途 |
|------|------|
| `tauri` | 桌面應用框架 |
| `image` | 圖片格式支援（PNG、WebP 等） |
| `fast_image_resize` | SIMD 加速 resize |
| `mozjpeg` | JPEG 高速 decode / encode |
| `rayon` | 多核心平行處理 |
| `tokio` | 非同步執行環境 |