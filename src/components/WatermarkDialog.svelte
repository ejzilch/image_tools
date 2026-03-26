<script>
    import { wmSettings, saveWmSettings } from "../stores/watermark.js";

    let { onSave, onClose } = $props();

    let wmText = $state($wmSettings.text);
    let wmFontName = $state($wmSettings.fontName);
    let wmPosition = $state($wmSettings.position);
    let wmOpacity = $state($wmSettings.opacity);
    let wmColor = $state($wmSettings.color);
    let wmFontSize = $state($wmSettings.fontSize);
    let wmBold = $state($wmSettings.bold);

    /** @type {HTMLCanvasElement} */
    let canvas;

    // 字型名稱對應 CSS font-family
    const fontFamilyMap = {
        notosanstc: "'NotoSansTC', sans-serif",
        chenyuluoyan: "'ChenYuLuoyan', serif",
        dancingscript: "'DancingScript', cursive",
    };

    // 預覽畫布尺寸
    const PREVIEW_W = 340;
    const PREVIEW_H = 220;

    // 把 hex + opacity% → rgba
    function hexToRgba(hex, opacityPercent) {
        let h = hex;
        if (h.length === 4) {
            h = `#${h[1]}${h[1]}${h[2]}${h[2]}${h[3]}${h[3]}`;
        }
        const r = parseInt(h.slice(1, 3), 16);
        const g = parseInt(h.slice(3, 5), 16);
        const b = parseInt(h.slice(5, 7), 16);
        // 100% → 完全透明，0% → 不透明
        const alpha = (100 - opacityPercent) / 100;
        return `rgba(${r},${g},${b},${alpha})`;
    }

    //  模擬 back-end measure_text_width
    function measureText(ctx, text) {
        return ctx.measureText(text).width;
    }

    //  模擬 back-end draw_text_on，含 bold 描邊
    function drawText(ctx, text, x, y, color, bold) {
        ctx.fillStyle = color;
        // 移除手動偏移迴圈，直接使用 ctx.font 裡的 bold 設定
        ctx.fillText(text, x, y);
    }

    // 模擬 back-end calc_watermark_position
    function calcPosition(position, imgW, imgH, textW, textH, margin) {
        switch (position) {
            case "top-left":
                return { x: margin, y: margin + textH };
            case "top-right":
                return { x: imgW - textW - margin, y: margin + textH };
            case "bottom-left":
                return { x: margin, y: imgH - margin };
            default:
                return { x: imgW - textW - margin, y: imgH - margin };
        }
    }

    function drawPreview() {
        if (!canvas) return;
        const ctx = canvas.getContext("2d");
        const dpr = window.devicePixelRatio || 1;

        // 1. 處理模糊：High DPI 設置
        canvas.width = PREVIEW_W * dpr;
        canvas.height = PREVIEW_H * dpr;
        ctx.scale(dpr, dpr);

        // 清空背景
        ctx.clearRect(0, 0, PREVIEW_W, PREVIEW_H);

        // 背景：灰色漸層假圖
        const grad = ctx.createLinearGradient(0, 0, PREVIEW_W, PREVIEW_H);
        grad.addColorStop(0, "#d0d0d0");
        grad.addColorStop(1, "#a0a0a0");
        ctx.fillStyle = grad;
        ctx.fillRect(0, 0, PREVIEW_W, PREVIEW_H);

        // 在假圖上畫幾條線表示內容
        ctx.strokeStyle = "rgba(255,255,255,0.25)";
        ctx.lineWidth = 1;
        for (let i = 30; i < PREVIEW_H; i += 30) {
            ctx.beginPath();
            ctx.moveTo(0, i);
            ctx.lineTo(PREVIEW_W, i);
            ctx.stroke();
        }

        if (!wmText) return;

        // 設置字體
        const scale = wmFontSize * (PREVIEW_W / 1000);
        const fontFamily = fontFamilyMap[wmFontName] || "sans-serif";
        const weight = wmBold ? "700" : "400";

        ctx.font = `${weight} ${scale}px ${fontFamily}`;
        ctx.textBaseline = "alphabetic";
        ctx.textAlign = "left";

        const color = hexToRgba(wmColor, wmOpacity);
        const textW = ctx.measureText(wmText).width;
        const textH = scale * 0.8; // 修正高度
        const margin = PREVIEW_W * 0.03;

        switch (wmPosition) {
            case "tiled":
                drawTiled(ctx, textW, scale, color);
                break;
            case "diagonal":
                drawDiagonal(ctx, textW, textH, scale, color);
                break;
            default: {
                const pos = calcPosition(
                    wmPosition,
                    PREVIEW_W,
                    PREVIEW_H,
                    textW,
                    textH,
                    margin,
                );
                drawText(ctx, wmText, pos.x, pos.y, color, wmBold);
            }
        }
    }

    // 模擬 back-end apply_tiled_watermark
    function drawTiled(ctx, textW, scale, color) {
        const imgW = PREVIEW_W;
        const imgH = PREVIEW_H;
        const textH = scale * 0.8; // 估算文字高度

        // 1. 想要有 5 排字，所以間隔有 4 個
        // 計算總可用高度：從 0 到 (imgH - textH)
        const rowCount = 5;
        const spacingY = (imgH - textH) / (rowCount - 1);

        const spacingX = textW * 3;
        const offset = spacingX / 2;

        ctx.textBaseline = "top";

        // 2. 這裡我們直接用 row index 來跑，確保精確控制排數
        for (let row = 0; row < rowCount; row++) {
            const y = row * spacingY;
            const xOff = row % 2 === 0 ? 0 : offset;

            // 水平方向維持原本的溢出繪製邏輯
            let x = -spacingX + xOff;
            while (x < imgW + spacingX) {
                drawText(ctx, wmText, x, y, color, wmBold);
                x += spacingX;
            }
        }
    }

    // 模擬 back-end  apply_diagonal_watermark
    function drawDiagonal(ctx, textW, textH, scale, color) {
        const imgW = PREVIEW_W;
        const imgH = PREVIEW_H;
        const angleRad = -Math.atan2(imgH, imgW);

        // 使用 scale 讓間距動態化
        const spacingX = textW + scale * 3;
        const spacingY = scale * 5;
        const offset = spacingX / 2;

        let row = 0;
        // 範圍擴大一點從 -imgH 開始，確保旋轉後角落不會空掉
        for (let y = -imgH; y < imgH * 1.5; y += spacingY) {
            const xOff = row % 2 === 0 ? 0 : offset;
            for (let x = -imgW; x < imgW * 1.5; x += spacingX) {
                ctx.save();

                // 移動到文字中心點
                ctx.translate(x + textW / 2, y + textH / 2);
                ctx.rotate(angleRad);

                drawText(ctx, wmText, -textW / 2, -textH / 2, color, wmBold);

                ctx.restore();
            }
            row++;
        }
    }

    // 每次參數變更重繪
    $effect(() => {
        // 追蹤所有變數
        const deps = [wmText, wmPosition, wmOpacity, wmColor];
        const currentFont = fontFamilyMap[wmFontName] || "sans-serif";
        const fontSizeStr = `${wmFontSize}px`;
        const fontSpec = `${wmBold ? "bold" : "normal"} ${fontSizeStr} ${currentFont}`;

        // 強制去讀取該字型
        document.fonts
            .load(fontSpec)
            .then(() => {
                // 確認字型加載完畢後 drawPreview
                requestAnimationFrame(drawPreview);
            })
            .catch((err) => {
                console.error("字型加載失敗:", err);
                // 失敗了就先用預設字展示
                requestAnimationFrame(drawPreview);
            });
    });

    async function save() {
        await saveWmSettings({
            text: wmText,
            fontName: wmFontName,
            position: wmPosition,
            opacity: wmOpacity,
            color: wmColor,
            fontSize: wmFontSize,
            bold: wmBold,
        });
        onSave();
    }

    function reset() {
        wmText = "";
        wmFontName = "notosanstc";
        wmPosition = "bottom-right";
        wmOpacity = 70;
        wmColor = "#ffffff";
        wmFontSize = 40;
        wmBold = false;
    }
</script>

<div class="overlay">
    <div class="dialog-box watermark-dialog">
        <p class="label">浮水印設定</p>

        <div class="dialog-body">
            <!-- 左：設定欄 -->
            <div class="settings-col">
                <div class="wm-field-row">
                    <label for="text">文字</label>
                    <input
                        type="text"
                        bind:value={wmText}
                        placeholder="© 2026 Your Name"
                        id="text"
                    />
                </div>

                <div class="wm-field-row">
                    <label for="font-select">字型</label>
                    <div class="select-wrapper">
                        <select bind:value={wmFontName} id="font-select">
                            <option value="notosanstc">NotoSansTC</option>
                            <option value="chenyuluoyan">ChenYuLuoyan</option>
                            <option value="dancingscript">DancingScript</option>
                        </select>
                    </div>
                </div>

                <div class="wm-field-row">
                    <label for="color">顏色</label>
                    <input type="color" bind:value={wmColor} id="color" />
                    <button
                        class="bold-btn"
                        class:active={wmBold}
                        onclick={() => (wmBold = !wmBold)}
                    >
                        <b>B</b>
                    </button>
                </div>

                <div class="wm-field-row">
                    <label for="range">透明度 </label>
                    <span class="val">{wmOpacity}%</span>
                    <input
                        type="range"
                        min="0"
                        max="100"
                        bind:value={wmOpacity}
                        id="range"
                    />
                </div>

                <div class="wm-field-row">
                    <label for="number">字體大小</label>
                    <input
                        type="number"
                        bind:value={wmFontSize}
                        min="10"
                        max="200"
                        id="number"
                    />
                    <span>px</span>
                </div>

                <div class="wm-field-row">
                    <label for="pos-select">位置</label>
                    <div class="select-wrapper">
                        <select bind:value={wmPosition} id="pos-select">
                            <option value="top-left">左上</option>
                            <option value="top-right">右上</option>
                            <option value="bottom-left">左下</option>
                            <option value="bottom-right">右下</option>
                            <option value="tiled">橫向滿版</option>
                            <option value="diagonal">斜向對角</option>
                        </select>
                    </div>
                </div>
            </div>

            <!-- 右：預覽欄 -->
            <div class="preview-col">
                <p class="preview-label">預覽</p>
                <canvas
                    bind:this={canvas}
                    width={340}
                    height={220}
                    class="preview-canvas"
                ></canvas>
                <p class="preview-hint">實際輸出大小依圖片尺寸縮放</p>
            </div>
        </div>

        <div class="dialog-buttons">
            <button class="btn-ok" onclick={save}>儲存</button>
            <button class="btn-cancel" onclick={reset}>重置</button>
            <button class="btn-cancel" onclick={onClose}>關閉</button>
        </div>
    </div>
</div>

<style>
    .overlay {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.4);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 100;
    }

    .dialog-box {
        background: var(--bg-secondary);
        color: var(--text);
        padding: 1.2rem;
        border-radius: 8px;
        box-shadow: 0 4px 16px var(--shadow-dialog);
    }

    .watermark-dialog {
        width: 700px;
        text-align: left;
    }

    .label {
        font-weight: bold;
        margin-bottom: 0.75rem;
    }

    /* 水平排列：設定 + 預覽 */
    .dialog-body {
        display: flex;
        gap: 1.5rem;
        align-items: flex-start;
    }

    .settings-col {
        flex: 0 0 320px;
    }

    .preview-col {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: flex-start;
    }

    .preview-label {
        font-weight: bold;
        margin-bottom: 0.4rem;
        font-size: 0.9rem;
    }

    .preview-canvas {
        width: 340px;
        height: 220px;
        border-radius: 6px;
        border: 1px solid var(--border);
        display: block;
    }

    .preview-hint {
        font-size: 0.75rem;
        color: var(--text-muted);
        margin-top: 0.4rem;
    }

    /* 原有欄位樣式 */
    .wm-field-row {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        margin-bottom: 0.75rem;
    }

    .wm-field-row label {
        min-width: 80px;
        text-align: left;
        white-space: nowrap;
    }

    .wm-field-row input[type="range"] {
        flex: 1;
    }

    .wm-field-row .val {
        flex: 0 0 50px;
        text-align: left;
        font-size: 0.9rem;
        font-variant-numeric: tabular-nums;
    }

    .wm-field-row input[type="text"] {
        flex: 1;
        padding: 0.25rem 0.5rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background-color: var(--bg-secondary);
        color: var(--text);
        font-size: 1rem;
    }

    .select-wrapper {
        position: relative;
        display: inline-block;
    }

    .select-wrapper::after {
        content: "";
        position: absolute;
        right: 0.5rem;
        top: 50%;
        transform: translateY(-50%);
        width: 0;
        height: 0;
        border-left: 4px solid transparent;
        border-right: 4px solid transparent;
        border-top: 6px solid var(--arrow);
        pointer-events: none;
    }

    select {
        appearance: none;
        -webkit-appearance: none;
        padding: 0.25rem 2rem 0.25rem 0.5rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background-color: var(--bg-secondary);
        color: var(--text);
        font-size: 1rem;
        cursor: pointer;
    }

    .dialog-buttons {
        display: flex;
        justify-content: center;
        gap: 1rem;
        margin-top: 1rem;
    }

    .btn-ok {
        padding: 0.4rem 1.5rem;
        background: var(--btn-primary-bg);
        color: var(--btn-primary-text);
        border: none;
        border-radius: 4px;
        cursor: pointer;
    }

    .btn-cancel {
        padding: 0.4rem 1.5rem;
        background: var(--btn-secondary-bg);
        border: none;
        border-radius: 4px;
        cursor: pointer;
    }

    .bold-btn {
        font-size: 1rem;
        padding: 0.2 0.6rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--bg-secondary);
        color: var(--text);
        cursor: pointer;
    }

    .bold-btn.active {
        background: var(--btn-primary-bg);
        color: var(--btn-primary-text);
        border-color: var(--btn-primary-bg);
        box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.3);
        font-weight: bold;
    }

    @media (prefers-color-scheme: dark) {
        .bold-btn.active {
            background: #ffffff;
            color: #000000;
            border-color: #ffffff;
        }
    }

    @font-face {
        font-family: "NotoSansTC";
        src: url("../src-tauri/fonts/NotoSansTC-VariableFont_wght.ttf")
            format("truetype");
    }

    @font-face {
        font-family: "ChenYuLuoyan";
        src: url("../src-tauri/fonts/Chenyuluoyan-Thin.ttf") format("truetype");
        font-weight: normal;
        font-style: normal;
    }

    @font-face {
        font-family: "dancingscript";
        src: url("../src-tauri/fonts/DancingScript-VariableFont_wght.ttf")
            format("truetype");
    }
</style>
