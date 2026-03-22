<script>
    import { createEventDispatcher } from "svelte";
    import { load } from "@tauri-apps/plugin-store";

    const dispatch = createEventDispatcher();

    let wmText = "";
    let wmFontName = "NotoSans";
    let wmPosition = "bottom-right";
    let wmOpacity = 70;
    let wmColor = "#ffffff";
    let wmFontSize = 40;
    let wmBold = false;

    // 載入已儲存的設定
    load("settings.json").then(async (store) => {
        const saved = await store.get("watermark");
        if (saved) {
            wmText = saved.text ?? "";
            wmFontName = saved.fontName ?? "NotoSans";
            wmPosition = saved.position ?? "bottom-right";
            wmOpacity = saved.opacity ?? 70;
            wmColor = saved.color ?? "#ffffff";
            wmFontSize = saved.fontSize ?? 40;
            wmBold = saved.bold ?? false;
        }
    });

    async function save() {
        const store = await load("settings.json");
        await store.set("watermark", {
            text: wmText,
            fontName: wmFontName,
            position: wmPosition,
            opacity: wmOpacity,
            color: wmColor,
            fontSize: wmFontSize,
            bold: wmBold,
        });
        await store.save();

        dispatch("save", {
            text: wmText,
            fontName: wmFontName,
            position: wmPosition,
            opacity: wmOpacity,
            color: wmColor,
            fontSize: wmFontSize,
            bold: wmBold,
        });
    }

    function reset() {
        wmText = "";
        wmFontName = "NotoSans";
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

        <div class="wm-field-row">
            <label for="text">文字</label>
            <input
                type="text"
                bind:value={wmText}
                placeholder="© 2026 Your Name"
            />
        </div>

        <div class="wm-field-row">
            <label for="select-wrapper">字型</label>
            <div class="select-wrapper">
                <select bind:value={wmFontName}>
                    <option value="NotoSans">Noto Sans</option>
                    <option value="NotoSerif">Noto Serif</option>
                    <option value="RobotoMono">Roboto Mono</option>
                </select>
            </div>
        </div>

        <div class="wm-field-row">
            <label for="color">顏色</label>
            <input type="color" bind:value={wmColor} />
            <button
                class="bold-btn"
                class:active={wmBold}
                on:click={() => (wmBold = !wmBold)}
            >
                <b>B</b>
            </button>
        </div>

        <div class="wm-field-row">
            <label for="range">透明度　{wmOpacity}%</label>
            <input type="range" min="0" max="100" bind:value={wmOpacity} />
        </div>

        <div class="wm-field-row">
            <label for="number">字體大小</label>
            <input type="number" bind:value={wmFontSize} min="10" max="200" />
            <span>px</span>
        </div>

        <div class="wm-field-row">
            <label for="select-wrapper">位置</label>
            <div class="select-wrapper">
                <select bind:value={wmPosition}>
                    <option value="top-left">左上</option>
                    <option value="top-right">右上</option>
                    <option value="bottom-left">左下</option>
                    <option value="bottom-right">右下</option>
                    <option value="tiled">橫向滿版</option>
                    <option value="diagonal">斜向對角</option>
                </select>
            </div>
        </div>

        <div class="dialog-buttons" style="margin-top: 1rem;">
            <button class="btn-ok" on:click={save}>儲存</button>
            <button class="btn-cancel" on:click={reset}>重置</button>
            <button class="btn-cancel" on:click={() => dispatch("close")}
                >關閉</button
            >
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
        width: 380px;
        text-align: left;
    }

    .label {
        font-weight: bold;
        margin-bottom: 0.5rem;
    }

    .wm-field-row {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        margin-bottom: 0.75rem;
    }

    .wm-field-row label {
        min-width: 80px;
        text-align: left;
    }

    .wm-field-row input[type="range"] {
        flex: 1;
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
        padding: 0.25rem 0.75rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--bg-secondary);
        color: var(--text);
        cursor: pointer;
        font-size: 1rem;
        transition: all 0.1s;
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
</style>
