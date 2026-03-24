<script>
  import { open } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { get } from "svelte/store";

  import AlertDialog from "./components/AlertDialog.svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";
  import WatermarkDialog from "./components/WatermarkDialog.svelte";
  import ProgressPanel from "./components/ProgressPanel.svelte";
  import FileListDialog from "./components/FileListDialog.svelte";
  import ShrinkPanel from "./components/ShrinkPanel.svelte";
  import CompressPanel from "./components/CompressPanel.svelte";
  import RenamePanel from "./components/RenamePanel.svelte";

  import { selectedFiles, addFiles } from "./stores/files.js";
  import { shrinkMode, width, height, ratio } from "./stores/shrink.js";
  import { targetSize, targetUnit } from "./stores/compress.js";
  import { renameText, renameMode } from "./stores/rename.js";
  import {
    isProcessing,
    isDone,
    progress,
    successCount,
    failedFiles,
    outputDirs,
  } from "./stores/progress.js";
  import { wmSettings } from "./stores/watermark.js";
  import { saveWmSettings } from "./stores/watermark.js";

  // 狀態
  let mode = $state("shrink");
  let showOptions = $state(false);
  let isDragging = $state(false);
  let showFileList = $state(false);

  // 進度
  let dragListeners = []; // 元件存活期間
  let progressListeners = []; // 只在執行期間

  // 浮水印設定
  let manualWatermark = $state(false);
  let enableWatermark = $derived(mode === "watermark" || manualWatermark);
  let showWatermarkSettings = $state(false);

  // 彈窗
  let alertMessage = $state("");
  let showAlert = $state(false);
  let closeAlert = $state((_value) => {});
  let confirmMessage = $state("");
  let showConfirm = $state(false);
  let resolveConfirm = $state((_value) => {});

  // 各個 mode 的驗證
  const validators = {
    shrink: validateShrink,
    compress: validateCompress,
    rename: validateRename,
    watermark: validateWatermark,
  };

  // 各個 mode 的 payload
  /** @type {Record<string, (files: string[]) => { command: string, payload: object }>} */
  const modeHandlers = {
    shrink: (files) => ({
      command: "shrink_image",
      payload: {
        inputs: files,
        shrinkMode: get(shrinkMode),
        width: Number(get(width)),
        height: Number(get(height)),
        ratio: Number(get(ratio)),
        watermark: enableWatermark ? buildWatermarkPayload() : null,
      },
    }),

    watermark: (files) => ({
      command: "watermark_only",
      payload: {
        inputs: files,
        watermark: buildWatermarkPayload(),
      },
    }),

    rename: (files) => ({
      command: "rename_images",
      payload: {
        inputs: files,
        customText: get(renameText).trim(),
        renameMode: get(renameMode),
        watermark: enableWatermark ? buildWatermarkPayload() : null,
      },
    }),

    compress: (files) => ({
      command: "compress_image",
      payload: {
        inputs: files,
        targetBytes:
          Number(get(targetSize)) *
          (get(targetUnit) === "kb" ? 1024 : 1024 * 1024),
        watermark: enableWatermark ? buildWatermarkPayload() : null,
      },
    }),
  };

  // 工具函式
  function toggleOptions() {
    showOptions = !showOptions;
  }

  /** @param {MouseEvent} e */
  function handleClickOutside(e) {
    if (!(/** @type {HTMLElement} */ (e.target).closest(".picker")))
      showOptions = false;
  }

  onMount(() => {
    listen("tauri://drag-drop", ({ payload }) => {
      addFiles(payload.paths ?? []);
      isDragging = false;
    }).then(addDragListener);

    listen("tauri://drag-over", () => {
      isDragging = true;
    }).then(addDragListener);

    listen("tauri://drag-leave", () => {
      isDragging = false;
    }).then(addDragListener);

    return () => {
      cleanupAllListeners();
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
      addFiles(files);
    }
  }

  async function pickFolder() {
    showOptions = false;
    const selected = await open({ multiple: false, directory: true });
    if (selected) {
      addFiles([selected]);
    }
  }

  /** @param {string} msg */
  async function showWarningDialog(msg) {
    alertMessage = msg;
    showAlert = true;
    await new Promise((resolve) => {
      closeAlert = resolve;
    });
  }

  /** @param {string} msg */
  async function showConfirmDialog(msg) {
    confirmMessage = msg;
    showConfirm = true;
    return await new Promise((resolve) => {
      resolveConfirm = resolve;
    });
  }

  function dismissAlert() {
    showAlert = false;
    closeAlert(undefined);
  }

  function confirmOk() {
    showConfirm = false;
    resolveConfirm(true);
  }

  function confirmCancel() {
    showConfirm = false;
    resolveConfirm(false);
  }

  async function validateShrink() {
    if (get(shrinkMode) === "dimension") {
      const w = Number(get(width));
      const h = Number(get(height));
      if (!w || w < 1 || w > 16383) {
        await showWarningDialog("寬度需介於 1 ~ 16383");
        return false;
      }
      if (!h || h < 1 || h > 16383) {
        await showWarningDialog("高度需介於 1 ~ 16383");
        return false;
      }
    } else {
      const r = Number(get(ratio));
      if (!r || r < 1 || r > 99) {
        await showWarningDialog("縮小比例需介於 1 ~ 99%");
        return false;
      }
    }
    return true;
  }

  async function validateCompress() {
    if (!get(targetSize) || Number(get(targetSize)) < 1) {
      await showWarningDialog("請輸入目標大小");
      return false;
    }
    return true;
  }

  async function validateRename() {
    if (!get(renameText).trim()) {
      await showWarningDialog("請輸入自訂文字");
      return false;
    }
    if (/[\\/:*?"<>|]/.test(get(renameText))) {
      await showWarningDialog('自訂文字不能包含特殊字元：\\ / : * ? " < > |');
      return false;
    }
    return true;
  }

  async function validateWatermark() {
    if (!get(wmSettings).text) {
      await showWarningDialog("請輸入浮水印文字");
      return false;
    }
    return true;
  }

  async function validate() {
    if (get(selectedFiles).length === 0) {
      await showWarningDialog("請先選擇檔案");
      return false;
    }

    if (enableWatermark || mode === "watermark") {
      console.log(enableWatermark);
      if (!(await validateWatermark())) return false;
    }

    if (mode !== "watermark") {
      const validator = validators[mode];
      if (validator && !(await validator())) return false;
    }

    return true;
  }

  async function confirmOverwrite() {
    const suffixMap = {
      shrink: "shrink",
      compress: "compress",
      watermark: "watermark",
      rename: "rename",
    };
    const suffix = suffixMap[mode];
    const files = get(selectedFiles);
    const dirExists = await invoke("check_output_dir_exists", {
      inputPath: files[0],
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

    cleanupProgressListeners(); // 清掉上一輪

    isProcessing.set(true);
    isDone.set(false);
    failedFiles.set([]);
    successCount.set(0);
    progress.set({ current: 0, total: 0 });

    addProgressListener(
      await listen("progress", ({ payload: p }) => {
        progress.set({ current: p.current, total: p.total });
        if (p.success) successCount.update((n) => n + 1);
        else
          failedFiles.update((arr) => [
            ...arr,
            { file: p.file, error: p.error },
          ]);
      }),
    );

    addProgressListener(
      await listen("progress_total", ({ payload }) => {
        progress.set({ current: 0, total: payload });
      }),
    );

    const files = get(selectedFiles);

    try {
      const handler = modeHandlers[mode];
      if (!handler) throw new Error(`未知模式：${mode}`);

      const { command, payload } = handler(files);
      const result = await invoke(command, payload);
      outputDirs.set(result.output_dirs);
    } catch (e) {
      failedFiles.update((arr) => [
        ...arr,
        { file: "未知錯誤", error: String(e) },
      ]);
    } finally {
      isProcessing.set(false);
      isDone.set(true);
      cleanupProgressListeners();
    }
  }

  async function cancel() {
    await invoke("cancel_processing");
    isProcessing.set(false);
    isDone.set(false);
    progress.set({ current: 0, total: 0 });
    successCount.set(0);
    failedFiles.set([]);
    cleanupProgressListeners();
  }

  /** @param {string} hex */
  function hexToRgb(hex) {
    // 處理縮寫色碼，例如 #fff → #ffffff
    if (hex.length === 4) {
      hex = `#${hex[1]}${hex[1]}${hex[2]}${hex[2]}${hex[3]}${hex[3]}`;
    }

    const r = parseInt(hex.slice(1, 3), 16);
    const g = parseInt(hex.slice(3, 5), 16);
    const b = parseInt(hex.slice(5, 7), 16);

    // 任何一個是 NaN 就回傳白色作為預設值
    if (isNaN(r) || isNaN(g) || isNaN(b)) {
      return [255, 255, 255];
    }

    return [r, g, b];
  }

  function buildWatermarkPayload() {
    return {
      text: get(wmSettings).text,
      position: get(wmSettings).position,
      opacity: (100 - get(wmSettings).opacity) / 100,
      color: hexToRgb(get(wmSettings).color),
      fontSize: get(wmSettings).fontSize,
      bold: get(wmSettings).bold,
    };
  }

  /** @param {() => void} fn */
  function addDragListener(fn) {
    dragListeners.push(fn);
  }

  /** @param {() => void} fn */
  function addProgressListener(fn) {
    progressListeners.push(fn);
  }

  function cleanupProgressListeners() {
    progressListeners.forEach((fn) => fn());
    progressListeners = [];
  }

  function cleanupAllListeners() {
    dragListeners.forEach((fn) => fn());
    dragListeners = [];
    progressListeners.forEach((fn) => fn());
    progressListeners = [];
  }
</script>

<svelte:window onclick={handleClickOutside} />

<main>
  {#if showAlert}
    <AlertDialog message={alertMessage} onClose={dismissAlert} />
  {/if}

  {#if showConfirm}
    <ConfirmDialog
      message={confirmMessage}
      onOk={confirmOk}
      onCancel={confirmCancel}
    />
  {/if}

  {#if showWatermarkSettings}
    <WatermarkDialog
      onSave={() => (showWatermarkSettings = false)}
      onClose={() => (showWatermarkSettings = false)}
    />
  {/if}

  <div class="layout">
    <!-- 左欄 -->
    <div class="col">
      <section>
        <p class="label">選擇輸入</p>
        <div class="select-wrapper picker">
          <button
            class="select-btn"
            onclick={(e) => {
              e.stopPropagation();
              toggleOptions();
            }}
          >
            選擇輸入
          </button>
          {#if showOptions}
            <div class="options">
              <button onclick={pickFiles}>選擇檔案</button>
              <button onclick={pickFolder}>新增資料夾</button>
            </div>
          {/if}
        </div>

        <!-- 拖曳區 -->
        <div class="drop-zone" class:drag-over={isDragging}>
          {isDragging ? "放開以加入" : "拖曳檔案或資料夾到這裡"}
        </div>

        {#if showFileList}
          <FileListDialog onClose={() => (showFileList = false)} />
        {/if}

        <!-- 拖曳區和檔案數量顯示改用 $selectedFiles -->
        {#if $selectedFiles.length > 0}
          <div class="file-summary">
            <span>已選擇 {$selectedFiles.length} 個檔案</span>
            <button class="view-btn" onclick={() => (showFileList = true)}
              >檢視</button
            >
          </div>
        {:else}
          <div class="selected">尚未選擇</div>
        {/if}

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
                bind:checked={manualWatermark}
                disabled={mode === "watermark"}
              />
              加入浮水印
            </label>
            {#if enableWatermark}
              <button
                class="wm-settings-btn"
                onclick={() => (showWatermarkSettings = true)}
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
      </section>
    </div>

    <!-- 右欄 -->
    <div class="col-right">
      {#if mode === "shrink"}
        <ShrinkPanel />
      {:else if mode === "compress"}
        <CompressPanel />
      {:else if mode === "rename"}
        <RenamePanel />
      {/if}

      <button class="execute" onclick={execute} disabled={$isProcessing}>
        {$isProcessing ? "處理中..." : "執行"}
      </button>

      <ProgressPanel onCancel={cancel} />
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

  .col-right {
    display: flex;
    flex-direction: column;
    height: 100%;
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
    border-radius: 4px;
    background: var(--bg-secondary);
    color: var(--text);
    cursor: pointer;
  }
</style>
