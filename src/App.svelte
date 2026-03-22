<script>
  import { open } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { load } from "@tauri-apps/plugin-store";
  import { onMount } from "svelte";

  // 狀態
  let selectedFiles = [];
  let mode = "shrink";
  let shrinkMode = "dimension";
  let showOptions = false;
  let width = "";
  let height = "";
  let ratio = "";
  let targetSize = "";
  let targetUnit = "kb";
  let isProcessing = false;
  let wmBold = false;
  let isDragging = false;
  let showFileList = false;
  let renameText = "";
  let renameMode = "sequence"; // "sequence" | "date_sequence"

  // 進度
  let progress = { current: 0, total: 0 };
  let successCount = 0;
  let failedFiles = [];
  let isDone = false;
  let unlistenFn = null;

  // 浮水印設定
  let enableWatermark = false;
  let showWatermarkSettings = false;
  let wmText = "";
  let wmFontName = "NotoSans";
  let wmPosition = "bottom-right";
  let wmOpacity = 70;
  let wmColor = "#ffffff";
  let wmFontSize = 40;

  // 彈窗
  let alertMessage = "";
  let showAlert = false;
  /** @type {(value?: unknown) => void} */
  let closeAlert = (_value) => {};

  let confirmMessage = "";
  let showConfirm = false;
  /** @type {(value?: unknown) => void} */
  let resolveConfirm = (_value) => {};

  // 工具函式
  function toggleOptions() {
    showOptions = !showOptions;
  }

  function handleClickOutside(e) {
    if (!e.target.closest(".picker")) showOptions = false;
  }

  onMount(() => {
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

    let unlistenDrop = null;
    let unlistenHover = null;
    let unlistenLeave = null;

    listen("tauri://drag-drop", ({ payload }) => {
      const paths = payload.paths ?? [];
      const newPaths = paths.filter((p) => !selectedFiles.includes(p));
      if (newPaths.length > 0) {
        selectedFiles = [...selectedFiles, ...newPaths];
      }
      isDragging = false;
    }).then((fn) => {
      unlistenDrop = fn;
    });

    listen("tauri://drag-over", () => {
      isDragging = true;
    }).then((fn) => {
      unlistenHover = fn;
    });

    listen("tauri://drag-leave", () => {
      isDragging = false;
    }).then((fn) => {
      unlistenLeave = fn;
    });

    return () => {
      unlistenDrop?.();
      unlistenHover?.();
      unlistenLeave?.();
    };
  });

  async function pickFiles() {
    showOptions = false;
    const selected = await open({
      multiple: true,
      directory: false,
      filters: [
        {
          name: "Images",
          extensions: ["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff"],
        },
      ],
    });
    if (selected) {
      const files = Array.isArray(selected) ? selected : [selected];
      const newFiles = files.filter((f) => !selectedFiles.includes(f));
      selectedFiles = [...selectedFiles, ...newFiles];
    }
  }

  async function pickFolder() {
    showOptions = false;
    const selected = await open({ multiple: false, directory: true });
    if (selected) {
      // 累加，避免重複
      if (!selectedFiles.includes(selected)) {
        selectedFiles = [...selectedFiles, selected];
      }
    }
  }

  function onShrinkModeChange() {
    if (shrinkMode === "dimension") {
      ratio = "";
    } else {
      width = "";
      height = "";
    }
  }

  /** @param {string} msg */
  async function showWarning(msg) {
    alertMessage = msg;
    showAlert = true;
    await new Promise((resolve) => {
      closeAlert = resolve;
    });
  }

  function dismissAlert() {
    showAlert = false;
    closeAlert(undefined);
  }

  async function showConfirmDialog(msg) {
    confirmMessage = msg;
    showConfirm = true;
    return await new Promise((resolve) => {
      resolveConfirm = resolve;
    });
  }

  function confirmOk() {
    showConfirm = false;
    resolveConfirm(true);
  }

  function confirmCancel() {
    showConfirm = false;
    resolveConfirm(false);
  }

  async function validate() {
    if (selectedFiles.length === 0) {
      await showWarning("請先選擇檔案");
      return false;
    }

    if (enableWatermark || mode === "watermark") {
      if (!wmText) {
        await showWarning("請輸入浮水印文字");
        return false;
      }
    }

    if (mode === "shrink") {
      if (shrinkMode === "dimension") {
        const w = Number(width),
          h = Number(height);
        if (!w || w < 1 || w > 16383) {
          await showWarning("寬度需介於 1 ~ 16383");
          return false;
        }
        if (!h || h < 1 || h > 16383) {
          await showWarning("高度需介於 1 ~ 16383");
          return false;
        }
      } else {
        const r = Number(ratio);
        if (!r || r < 1 || r > 99) {
          await showWarning("縮小比例需介於 1 ~ 99%");
          return false;
        }
      }
    }

    if (mode === "compress" && (!targetSize || Number(targetSize) < 1)) {
      await showWarning("請輸入目標大小");
      return false;
    }

    if (mode === "rename") {
      if (!renameText.trim()) {
        await showWarning("請輸入自訂文字");
        return false;
      }
      // 防止特殊字元造成檔名錯誤
      if (/[\\/:*?"<>|]/.test(renameText)) {
        await showWarning('自訂文字不能包含特殊字元：\\ / : * ? " < > |');
        return false;
      }
    }

    return true;
  }

  async function confirmOverwrite() {
    const suffix =
      mode === "shrink"
        ? "shrink"
        : mode === "compress"
          ? "compress"
          : mode === "watermark"
            ? "watermark"
            : "rename";
    const dirExists = await invoke("check_output_dir_exists", {
      inputPath: selectedFiles[0],
      suffix,
    });
    if (dirExists) {
      return await showConfirmDialog("輸出資料夾已存在，是否覆蓋？");
    }
    return true;
  }

  async function execute() {
    if (!(await validate())) return;
    if (!(await confirmOverwrite())) return;

    isProcessing = true;
    isDone = false;
    failedFiles = [];
    successCount = 0;
    progress = { current: 0, total: selectedFiles.length };

    if (unlistenFn) unlistenFn();
    unlistenFn = await listen("progress", ({ payload: p }) => {
      progress = { current: p.current, total: p.total };
      if (p.success) successCount += 1;
      else failedFiles = [...failedFiles, { file: p.file, error: p.error }];
    });

    const unlistenTotal = await listen("progress_total", ({ payload }) => {
      progress = { current: 0, total: payload };
      unlistenTotal(); // 只需要聽一次
    });

    try {
      if (mode === "shrink") {
        await invoke("shrink_image", {
          inputs: selectedFiles,
          shrinkMode,
          width: Number(width),
          height: Number(height),
          ratio: Number(ratio),
          watermark: enableWatermark
            ? {
                text: wmText,
                position: wmPosition,
                opacity: (100 - wmOpacity) / 100,
                color: hexToRgb(wmColor),
                fontSize: wmFontSize,
                bold: wmBold,
              }
            : null,
        });
      } else if (mode === "watermark") {
        await invoke("watermark_only", {
          inputs: selectedFiles,
          watermark: {
            text: wmText,
            position: wmPosition,
            opacity: (100 - wmOpacity) / 100,
            color: hexToRgb(wmColor),
            fontSize: wmFontSize,
            bold: wmBold,
          },
        });
      } else if (mode === "rename") {
        await invoke("rename_images", {
          inputs: selectedFiles,
          customText: renameText.trim(),
          renameMode,
          watermark: enableWatermark
            ? {
                text: wmText,
                position: wmPosition,
                opacity: (100 - wmOpacity) / 100,
                color: hexToRgb(wmColor),
                fontSize: wmFontSize,
                bold: wmBold,
              }
            : null,
        });
      } else {
        await invoke("compress_image", {
          inputs: selectedFiles,
          targetBytes:
            Number(targetSize) * (targetUnit === "kb" ? 1024 : 1024 * 1024),
          watermark: enableWatermark
            ? {
                text: wmText,
                position: wmPosition,
                opacity: (100 - wmOpacity) / 100,
                color: hexToRgb(wmColor),
                fontSize: wmFontSize,
                bold: wmBold,
              }
            : null,
        });
      }
    } catch (e) {
      failedFiles = [...failedFiles, { file: "未知錯誤", error: String(e) }];
    } finally {
      isProcessing = false;
      isDone = true;
      unlistenFn?.();
      unlistenFn = null;
    }
  }

  async function cancel() {
    await invoke("cancel_processing");
    isProcessing = false;
    isDone = false;
    progress = { current: 0, total: 0 };
  }

  async function saveWatermarkSettings() {
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
    showWatermarkSettings = false;
  }

  function resetWatermarkSettings() {
    wmText = "";
    wmFontName = "NotoSans";
    wmPosition = "bottom-right";
    wmOpacity = 70;
    wmColor = "#ffffff";
    wmFontSize = 40;
  }

  function hexToRgb(hex) {
    return [
      parseInt(hex.slice(1, 3), 16),
      parseInt(hex.slice(3, 5), 16),
      parseInt(hex.slice(5, 7), 16),
    ];
  }

  function removeFile(index) {
    selectedFiles = selectedFiles.filter((_, i) => i !== index);
  }

  // Reactive
  $: selectedLabel =
    selectedFiles.length === 0
      ? "尚未選擇"
      : selectedFiles.length === 1
        ? selectedFiles[0].split("\\").pop().split("/").pop()
        : `已選擇 ${selectedFiles.length} 個檔案`;

  $: progressPct =
    progress.total > 0 ? (progress.current / progress.total) * 100 : 0;

  $: if (mode === "watermark") {
    enableWatermark = true;
  } else {
    enableWatermark = false;
  }
</script>

<svelte:window on:click={handleClickOutside} />

<main>
  {#if showAlert}
    <div class="overlay">
      <div class="dialog-box">
        <p>{alertMessage}</p>
        <button class="btn-ok" on:click={dismissAlert}>確定</button>
      </div>
    </div>
  {/if}

  <!-- 警告彈窗 -->
  {#if showConfirm}
    <div class="overlay">
      <div class="dialog-box">
        <p>{confirmMessage}</p>
        <div class="dialog-buttons">
          <button class="btn-ok" on:click={confirmOk}>確定</button>
          <button class="btn-cancel" on:click={confirmCancel}>取消</button>
        </div>
      </div>
    </div>
  {/if}

  {#if showWatermarkSettings}
    <div class="overlay">
      <div class="dialog-box watermark-dialog">
        <p class="label">浮水印設定</p>

        <div class="wm-field-row">
          <label for="wm-opacity">文字</label>
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
          <button class="btn-ok" on:click={saveWatermarkSettings}>儲存</button>
          <button class="btn-cancel" on:click={resetWatermarkSettings}
            >重置</button
          >
          <button
            class="btn-cancel"
            on:click={() => (showWatermarkSettings = false)}>關閉</button
          >
        </div>
      </div>
    </div>
  {/if}

  {#if showFileList}
    <div class="overlay">
      <div class="dialog-box file-list-dialog">
        <p class="label">已選擇的檔案（共 {selectedFiles.length} 個）</p>
        <ul class="file-list-modal">
          {#each selectedFiles as f, i}
            <li>
              <span title={f}>{f.split("\\").pop().split("/").pop()}</span>
              <button class="remove-btn" on:click={() => removeFile(i)}
                >✕</button
              >
            </li>
          {/each}
        </ul>
        <div class="dialog-buttons" style="margin-top: 1rem;">
          <button class="btn-ok" on:click={() => (showFileList = false)}
            >關閉</button
          >
          <button
            class="btn-cancel"
            on:click={() => {
              selectedFiles = [];
              showFileList = false;
            }}>清除全部</button
          >
        </div>
      </div>
    </div>
  {/if}

  <div class="layout">
    <!-- 左欄 -->
    <div class="col">
      <section>
        <p class="label">選擇輸入</p>
        <div class="select-wrapper picker">
          <button class="select-btn" on:click|stopPropagation={toggleOptions}>
            選擇輸入
          </button>
          {#if showOptions}
            <div class="options">
              <button on:click={pickFiles}>選擇檔案</button>
              <button on:click={pickFolder}>新增資料夾</button>
            </div>
          {/if}
        </div>

        <!-- 拖曳區 -->
        <div class="drop-zone" class:drag-over={isDragging}>
          {isDragging ? "放開以加入" : "拖曳檔案或資料夾到這裡"}
        </div>

        <!-- 已選清單 -->
        {#if selectedFiles.length > 0}
          <div class="file-summary">
            <span>已選擇 {selectedFiles.length} 個檔案</span>
            <button class="view-btn" on:click={() => (showFileList = true)}
              >檢視</button
            >
          </div>
        {:else}
          <div class="selected">尚未選擇</div>
        {/if}
      </section>

      <section>
        <p class="label">選擇功能</p>
        <div class="radio-group">
          <label
            ><input type="radio" bind:group={mode} value="shrink" /> 縮圖</label
          >
          <label
            ><input type="radio" bind:group={mode} value="compress" /> 壓縮</label
          >
          <label
            ><input type="radio" bind:group={mode} value="rename" /> 批量更名</label
          >
        </div>
        <div class="radio-group">
          <label
            ><input type="radio" bind:group={mode} value="watermark" /> 浮水印</label
          >
        </div>
      </section>

      <section>
        <p class="label">浮水印</p>
        <div class="wm-row">
          <label class="checkbox-label">
            <input
              type="checkbox"
              bind:checked={enableWatermark}
              disabled={mode === "watermark"}
            />
            加入浮水印
          </label>
          {#if enableWatermark}
            <button
              class="wm-settings-btn"
              on:click={() => (showWatermarkSettings = true)}
            >
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
              >
                <circle cx="12" cy="12" r="3" />
                <path
                  d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
                />
              </svg> 設定
            </button>
          {/if}
        </div>
      </section>
    </div>

    <!-- 右欄 -->
    <div class="col-right">
      {#if mode !== "watermark"}
        <section>
          <p class="label">參數設定</p>

          {#if mode === "shrink"}
            <div class="radio-group">
              <label
                ><input
                  type="radio"
                  bind:group={shrinkMode}
                  value="dimension"
                  on:change={onShrinkModeChange}
                /> 指定寬高</label
              >
              <label
                ><input
                  type="radio"
                  bind:group={shrinkMode}
                  value="ratio"
                  on:change={onShrinkModeChange}
                /> 維持比例縮小</label
              >
            </div>

            {#if shrinkMode === "dimension"}
              <div class="fields-row">
                <label
                  >寬度　<input
                    type="number"
                    bind:value={width}
                    min="1"
                    max="16383"
                  />（px）</label
                >
                <label
                  >高度　<input
                    type="number"
                    bind:value={height}
                    min="1"
                    max="16383"
                  />（px）</label
                >
              </div>
            {:else}
              <div class="fields-row">
                <label
                  >縮小至　<input
                    type="number"
                    bind:value={ratio}
                    min="1"
                    max="99"
                  />（%）</label
                >
              </div>
            {/if}
          {/if}
          {#if mode === "compress"}
            <div class="field-row">
              <label for="target-size">目標大小</label>
              <input
                id="target-size"
                type="number"
                bind:value={targetSize}
                min="1"
              />
              <div class="select-wrapper">
                <select bind:value={targetUnit}>
                  <option value="kb">KB</option>
                  <option value="mb">MB</option>
                </select>
              </div>
            </div>
          {/if}
          {#if mode === "rename"}
            <div class="field-row">
              <label for="rename-text">自訂文字</label>
              <input
                id="rename-text"
                type="text"
                bind:value={renameText}
                placeholder="photo"
              />
            </div>

            <div class="rename-options">
              <label>
                <input type="radio" bind:group={renameMode} value="sequence" />
                流水號
              </label>
              <label>
                <input
                  type="radio"
                  bind:group={renameMode}
                  value="date_sequence"
                />
                日期＋流水號
              </label>
              <p class="rename-preview">
                {renameText || "photo"}_{renameMode === "date_sequence"
                  ? "2026-03-22_"
                  : ""}00001.jpg
              </p>
            </div>
          {/if}
        </section>
      {/if}
      <button class="execute" on:click={execute} disabled={isProcessing}>
        {isProcessing ? "處理中..." : "執行"}
      </button>

      {#if isProcessing || isDone}
        <section class="results">
          {#if isProcessing}
            <p class="label">處理中... {progress.current} / {progress.total}</p>
            <div class="progress-bar-bg">
              <div
                class="progress-bar-fill"
                style="width: {progressPct}%"
              ></div>
            </div>
            <button class="cancel" on:click={cancel}>取消</button>
          {/if}

          {#if isDone}
            <p class="label">完成</p>
            <p class="success">✅ 成功：{successCount} 個</p>
            {#if failedFiles.length > 0}
              <p class="fail">❌ 失敗：{failedFiles.length} 個</p>
              {#each failedFiles as f}
                <p class="fail-detail">{f.file}：{f.error}</p>
              {/each}
            {/if}
          {/if}
        </section>
      {/if}
    </div>
  </div>
</main>

<style>
  /* CSS 亮色預設 */
  :global(:root) {
    --bg: #ffffff;
    --bg-secondary: #ffffff;
    --bg-hover: #f0f0f0;
    --text: #222222;
    --text-muted: #555555;
    --border: #cccccc;
    --border-hover: #888888;
    --border-focus: #333333;
    --arrow: #555555;
    --shadow: rgba(0, 0, 0, 0.15);
    --shadow-dialog: rgba(0, 0, 0, 0.2);
    --btn-primary-bg: #333333;
    --btn-primary-text: #ffffff;
    --btn-secondary-bg: #eeeeee;
    --progress-bg: #eeeeee;
  }

  /* 深色模式：只改變數值 */
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #1e1e1e;
      --bg-secondary: #2c2c2c;
      --bg-hover: #3a3a3a;
      --text: #e0e0e0;
      --text-muted: #aaaaaa;
      --border: #555555;
      --border-hover: #aaaaaa;
      --border-focus: #e0e0e0;
      --arrow: #aaaaaa;
      --shadow: rgba(0, 0, 0, 0.5);
      --shadow-dialog: rgba(0, 0, 0, 0.6);
      --btn-primary-bg: #555555;
      --btn-primary-text: #ffffff;
      --btn-secondary-bg: #3a3a3a;
      --progress-bg: #444444;
    }
  }

  /* 基本 CSS */
  main {
    padding: 1rem;
    font-family: sans-serif;
    display: flex;
    flex-direction: column;
    height: 100vh;
    box-sizing: border-box;
    background-color: var(--bg);
    color: var(--text);
  }

  .layout {
    display: grid;
    grid-template-columns: 0.7fr 1.3fr;
    gap: 2rem;
    flex: 1;
    align-items: stretch;
  }

  .col {
    border-right: 1px solid var(--border);
    padding-right: 1.5rem;
    min-width: 0; /* 防止撐開 grid */
    overflow: hidden; /* 加這行 */
    width: 240px; /* 明確固定寬度 */
  }

  section {
    margin-bottom: 1.5rem;
    text-align: left;
  }

  .label {
    font-weight: bold;
    margin-bottom: 0.5rem;
    text-align: left;
  }

  .radio-group {
    display: flex;
    gap: 1rem;
    margin-bottom: 0.75rem;
    font-size: 1rem;
  }

  .fields-row {
    display: flex;
    gap: 1rem;
    margin-top: 0.75rem;
  }

  .field-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-top: 0.75rem;
  }

  .col-right {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  /* input */
  input[type="number"],
  select,
  .select-btn {
    padding: 0.25rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    background-color: var(--bg-secondary);
    color: var(--text);
    font-size: 1rem;
    cursor: pointer;
  }

  input[type="number"] {
    width: 80px;
  }

  input[type="number"]:focus,
  select:focus,
  .select-btn:focus {
    outline: none;
    border-color: var(--border-focus);
  }

  input[type="number"]:hover,
  select:hover,
  .select-btn:hover {
    border-color: var(--border-hover);
  }

  /* Select wrapper */
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
    padding-right: 2rem;
  }

  .select-btn {
    appearance: none;
    -webkit-appearance: none;
    min-width: 100px;
    text-align: left;
    padding-right: 2rem;
  }

  /* Picker 下拉 */
  .picker {
    position: relative;
    display: inline-block;
  }

  .options {
    position: absolute;
    top: 100%;
    left: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 4px;
    overflow: hidden;
    z-index: 10;
    min-width: 120px;
    box-shadow: 0 2px 8px var(--shadow);
  }

  .options button {
    padding: 0.5rem 1rem;
    border: none;
    border-radius: 0;
    text-align: left;
    background: var(--bg-secondary);
    color: var(--text);
  }

  .options button:hover {
    background: var(--bg-hover);
  }

  /* 按鈕 */
  button {
    background-color: var(--bg-secondary);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 4px;
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .execute {
    width: 100%;
    padding: 0.75rem;
    font-size: 1rem;
    margin-top: auto;
  }

  .cancel {
    width: 100%;
    padding: 0.75rem;
    font-size: 1rem;
    margin-top: 0.5rem;
    background: #e53935;
    color: white;
    border: none;
    cursor: pointer;
  }

  .btn-ok {
    padding: 0.4rem 1.5rem;
    background: var(--btn-primary-bg);
    color: var(--btn-primary-text);
    border: none;
  }

  .btn-cancel {
    padding: 0.4rem 1.5rem;
    background: var(--btn-secondary-bg);
    border: none;
  }

  /* 彈窗 */
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
    min-width: 250px;
    text-align: center;
    box-shadow: 0 4px 16px var(--shadow-dialog);
  }

  .dialog-box p {
    margin-bottom: 1rem;
  }

  .dialog-buttons {
    display: flex;
    justify-content: center;
    gap: 1rem;
  }

  /* 進度條 */
  .results {
    margin-top: 1rem;
    text-align: left;
  }

  .progress-bar-bg {
    width: 100%;
    height: 10px;
    background: var(--progress-bg);
    border-radius: 5px;
    overflow: hidden;
    margin-top: 0.5rem;
  }

  .progress-bar-fill {
    height: 100%;
    background: #4caf50;
    border-radius: 5px;
    transition: width 0.2s ease;
  }

  /* 結果文字 */
  .success {
    color: #4caf50;
  }
  .fail {
    color: #e53935;
  }
  .fail-detail {
    color: #c0392b;
    font-size: 0.9rem;
  }

  /* 浮水印 */
  .watermark-dialog {
    width: 380px;
    text-align: left;
    z-index: 101;
    position: relative;
  }

  .wm-field-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 0.75rem;
  }

  .wm-field-row label {
    height: 2rem;
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

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    cursor: pointer;
  }

  .wm-settings-btn {
    padding: 0.25rem 0.75rem;
    font-size: 0.9rem;
    height: 2rem;
    box-sizing: border-box;
  }

  .wm-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    height: 2rem;
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

  .drop-zone {
    margin-top: 0.5rem;
    border: 2px dashed var(--border);
    border-radius: 6px;
    padding: 0.75rem 1rem;
    font-size: 0.9rem;
    color: var(--text-muted);
    text-align: center;
    transition: all 0.15s;
    cursor: default;
  }

  .drop-zone.drag-over {
    border-color: var(--border-focus);
    background: var(--bg-hover);
    color: var(--text);
  }

  .selected,
  .file-summary {
    height: 2rem;
    display: flex;
    align-items: center;
    margin: 0.5rem 0;
    font-size: 1rem;
    color: var(--text-muted);
  }

  .file-summary {
    justify-content: space-between;
  }

  .view-btn {
    font-size: 1rem;
    padding: 0.2 0.6rem;
    border: 1px solid var(--border);
    border-radius: 4=px;
    background: var(--bg-secondary);
    color: var(--text);
    cursor: pointer;
  }

  .file-list-dialog {
    width: 420px;
    text-align: left;
  }

  .file-list-modal {
    list-style: none;
    padding: 0;
    margin: 0;
    max-height: 270px;
    overflow-y: auto;
  }

  .file-list-modal li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.35rem 0;
    border-bottom: 1px solid var(--border);
    gap: 0.5rem;
  }

  .file-list-modal li span {
    font-size: 0.9rem;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .remove-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 0.8rem;
    padding: 0 0.25rem;
    flex-shrink: 0;
  }

  .remove-btn:hover {
    color: #e53935;
  }

  .rename-options {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-top: 0.75rem;
  }

  .rename-options label {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 1rem;
  }

  .rename-preview {
    width: 100%; /* 強制換行到下一列 */
    font-size: 0.9rem;
    color: var(--text-muted);
    font-family: monospace;
    margin: 0;
    padding: 0;
  }
</style>
