<script>
  import { open } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

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

  // 進度
  let progress = { current: 0, total: 0 };
  let successCount = 0;
  let failedFiles = [];
  let isDone = false;
  let unlistenFn = null;

  // 彈窗
  let alertMessage = "";
  let showAlert = false;
  let closeAlert = () => {};

  let confirmMessage = "";
  let showConfirm = false;
  let resolveConfirm = () => {};

  // 工具函式
  function toggleOptions() {
    showOptions = !showOptions;
  }

  function handleClickOutside(e) {
    if (!e.target.closest(".picker")) showOptions = false;
  }

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
    if (selected)
      selectedFiles = Array.isArray(selected) ? selected : [selected];
  }

  async function pickFolder() {
    showOptions = false;
    const selected = await open({ multiple: false, directory: true });
    if (selected) selectedFiles = [selected];
  }

  function onShrinkModeChange() {
    if (shrinkMode === "dimension") {
      ratio = "";
    } else {
      width = "";
      height = "";
    }
  }

  async function showWarning(msg) {
    alertMessage = msg;
    showAlert = true;
    await new Promise((resolve) => {
      closeAlert = resolve;
    });
  }

  function dismissAlert() {
    showAlert = false;
    closeAlert();
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

    return true;
  }

  async function confirmOverwrite() {
    const suffix = mode === "shrink" ? "shrink" : "compress";
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
        });
      } else {
        await invoke("compress_image", {
          inputs: selectedFiles,
          targetBytes:
            Number(targetSize) * (targetUnit === "kb" ? 1024 : 1024 * 1024),
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

  // Reactive
  $: selectedLabel =
    selectedFiles.length === 0
      ? "尚未選擇"
      : selectedFiles.length === 1
        ? selectedFiles[0].split("\\").pop().split("/").pop()
        : `已選擇 ${selectedFiles.length} 個檔案`;

  $: progressPct =
    progress.total > 0 ? (progress.current / progress.total) * 100 : 0;
</script>

<svelte:window on:click={handleClickOutside} />

<main>
  <!-- 警告彈窗 -->
  {#if showAlert}
    <div class="overlay">
      <div class="dialog-box">
        <p>{alertMessage}</p>
        <button class="btn-ok" on:click={dismissAlert}>確定</button>
      </div>
    </div>
  {/if}

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
              <button on:click={pickFolder}>選擇資料夾</button>
            </div>
          {/if}
        </div>
        <p class="selected">已選擇：{selectedLabel}</p>
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
        </div>
      </section>
    </div>

    <!-- 右欄 -->
    <div class="col-right">
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
        {:else}
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
      </section>

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

  .selected {
    margin-top: 0.5rem;
    font-size: 1rem;
    color: var(--text-muted);
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
    padding: 1.5rem;
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
</style>
