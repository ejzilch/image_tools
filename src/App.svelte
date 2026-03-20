<script>
  import { open, ask } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  let progress = { current: 0, total: 0 };
  let failedFiles = [];
  let successCount = 0;
  let isDone = false;
  let unlistenFn = null;

  let selectedFiles = [];
  let mode = "shrink";
  let shrinkMode = "dimension";
  let showOptions = false;

  let width = "";
  let height = "";
  let ratio = "";

  let quality = "";

  let results = [];
  let isProcessing = false;

  let targetSize = "";
  let targetUnit = "kb";

  let errors = { width: "", height: "", ratio: "", quality: "" };

  function toggleOptions() {
    showOptions = !showOptions;
  }

  function handleClickOutside(e) {
    if (!e.target.closest(".picker")) {
      showOptions = false;
    }
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
    if (selected) {
      selectedFiles = Array.isArray(selected) ? selected : [selected];
    }
  }

  async function pickFolder() {
    showOptions = false;
    const selected = await open({
      multiple: false,
      directory: true,
    });
    if (selected) {
      selectedFiles = [selected];
    }
  }

  $: selectedLabel =
    selectedFiles.length === 0
      ? "尚未選擇"
      : selectedFiles.length === 1
        ? selectedFiles[0].split("\\").pop().split("/").pop()
        : `已選擇 ${selectedFiles.length} 個檔案`;

  function onShrinkModeChange() {
    if (shrinkMode === "dimension") {
      ratio = "";
      errors.ratio = "";
    } else {
      width = "";
      height = "";
      errors.width = "";
      errors.height = "";
    }
  }

  async function validate() {
    let valid = true;

    if (selectedFiles.length === 0) {
      await showWarning("請先選擇檔案");
      return false;
    }

    const w = Number(width);
    const h = Number(height);
    const r = Number(ratio);
    const q = Number(quality);

    if (mode === "shrink") {
      if (shrinkMode === "dimension") {
        if (!w || w < 1 || w > 16383) {
          await showWarning("寬度需介於 1 ~ 16383");
          return false;
        }
        if (!h || h < 1 || h > 16383) {
          await showWarning("高度需介於 1 ~ 16383");
          return false;
        }
      } else {
        if (!r || r < 1 || r > 99) {
          await showWarning("縮小比例需介於 1 ~ 99%");
          return false;
        }
      }
    }

    if (mode === "compress") {
      if (!targetSize || Number(targetSize) < 1) {
        await showWarning("請輸入目標大小");
        return false;
      }
    }
    return valid;
  }

  let alertMessage = "";
  let showAlert = false;
  let closeAlert = (_) => {};

  async function showWarning(msg) {
    alertMessage = msg;
    showAlert = true;
    await new Promise((resolve) => {
      closeAlert = resolve;
    });
    return false;
  }

  function dismissAlert() {
    showAlert = false;
    closeAlert();
  }

  let confirmMessage = "";
  let showConfirm = false;
  let resolveConfirm = (_) => {};

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

  async function execute() {
    const valid = await validate();
    if (!valid) return;

    isProcessing = true;
    isDone = false;
    results = [];
    failedFiles = [];
    successCount = 0;
    progress = { current: 0, total: selectedFiles.length };

    // 監聽進度事件
    if (unlistenFn) unlistenFn();
    unlistenFn = await listen("progress", (event) => {
      const p = event.payload;
      progress = { current: p.current, total: p.total };
      if (p.success) {
        successCount += 1;
      } else {
        failedFiles = [...failedFiles, { file: p.file, error: p.error }];
      }
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
            targetUnit === "kb"
              ? Number(targetSize) * 1024
              : Number(targetSize) * 1024 * 1024,
        });
      }
    } catch (e) {
      failedFiles = [...failedFiles, { file: "未知錯誤", error: String(e) }];
    } finally {
      isProcessing = false;
      isDone = true;
      if (unlistenFn) {
        unlistenFn();
        unlistenFn = null;
      }
    }
  }
</script>

<!-- 點外面關閉選單 -->
<svelte:window on:click={handleClickOutside} />

<main>
  <!-- 自訂彈窗 -->
  {#if showAlert}
    <div class="overlay">
      <div class="alert-box">
        <p>{alertMessage}</p>
        <button on:click={dismissAlert}>確定</button>
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
    <!-- 左欄：選擇輸入 + 選擇功能 -->
    <div class="col">
      <section>
        <p class="label">選擇輸入</p>
        <div class="picker">
          <button on:click|stopPropagation={toggleOptions}> 選擇輸入 ▾ </button>
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
          <label>
            <input type="radio" bind:group={mode} value="shrink" /> 縮圖
          </label>
          <label>
            <input type="radio" bind:group={mode} value="compress" /> 壓縮
          </label>
        </div>
      </section>
    </div>

    <!-- 右欄：參數設定 + 執行按鈕 -->
    <div class="col-right">
      <section>
        <p class="label">參數設定</p>

        {#if mode === "shrink"}
          <div class="radio-group">
            <label>
              <input
                type="radio"
                bind:group={shrinkMode}
                value="dimension"
                on:change={onShrinkModeChange}
              />
              指定寬高
            </label>
            <label>
              <input
                type="radio"
                bind:group={shrinkMode}
                value="ratio"
                on:change={onShrinkModeChange}
              />
              維持比例縮小
            </label>
          </div>

          {#if shrinkMode === "dimension"}
            <div class="fields-row">
              <div class="field">
                <label for="width"
                  >寬度　<input
                    id="width"
                    type="number"
                    bind:value={width}
                    min="1"
                    max="16383"
                  />（px）</label
                >
              </div>
              <div class="field">
                <label for="height"
                  >高度　<input
                    id="height"
                    type="number"
                    bind:value={height}
                    min="1"
                    max="16383"
                  />（px）</label
                >
              </div>
            </div>
          {:else}
            <div class="fields-row">
              <div class="field">
                <label for="ratio"
                  >縮小至　<input
                    id="ratio"
                    type="number"
                    bind:value={ratio}
                    min="1"
                    max="99"
                  />（%）</label
                >
              </div>
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

      <!-- 執行按鈕在右欄底部 -->
      <button class="execute" on:click={execute} disabled={isProcessing}>
        {isProcessing ? "處理中..." : "執行"}
      </button>

      <!-- 結果 -->
      {#if isProcessing || isDone}
        <section class="results">
          {#if isProcessing}
            <p class="label">處理中... {progress.current} / {progress.total}</p>
            <div class="progress-bar-bg">
              <div
                class="progress-bar-fill"
                style="width: {progress.total > 0
                  ? (progress.current / progress.total) * 100
                  : 0}%"
              ></div>
            </div>
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
  main {
    padding: 1rem;
    font-family: sans-serif;
    display: flex;
    flex-direction: column;
    height: 100vh;
    box-sizing: border-box;
    text-align: left;
  }

  .layout {
    display: grid;
    grid-template-columns: 0.7fr 1.3fr;
    gap: 2rem;
    flex: 1;
    align-items: stretch;
  }

  .col {
    border-right: 1px solid #eee;
    padding-right: 1.5rem;
  }

  .col:last-child {
    border-right: none;
  }

  section {
    margin-bottom: 1.5rem;
    text-align: left;
  }

  .label {
    font-weight: bold;
    margin-bottom: 0.5rem;
  }

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
    background: white;
    border: 1px solid #ccc;
    border-radius: 4px;
    overflow: hidden;
    z-index: 10;
    min-width: 120px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  }

  .options button {
    padding: 0.5rem 1rem;
    border: none;
    border-radius: 0;
    text-align: left;
    cursor: pointer;
    background: white;
  }

  .options button:hover {
    background: #f0f0f0;
  }

  .selected {
    margin-top: 0.5rem;
    font-size: 1rem;
    color: #555;
  }

  .radio-group {
    display: flex;
    gap: 1rem;
    margin-bottom: 0.75rem;
    font-size: 1rem;
  }

  .field {
    display: flex;
    flex-direction: column;
    margin-top: 0.75rem;
  }

  .fields-row {
    display: flex;
    flex-direction: row;
    gap: 1rem;
    margin-top: 0.75rem;
  }

  .col-right {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .execute {
    width: 100%;
    padding: 0.75rem;
    font-size: 1rem;
    margin-top: auto; /* 自動推到最底 */
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  input[type="number"] {
    width: 80px;
    padding: 0.25rem;
  }

  .results {
    margin-top: 1rem;
  }

  .success {
    color: green;
  }
  .fail {
    color: red;
  }

  .overlay {
    position: fixed;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .alert-box {
    background: white;
    padding: 1.5rem;
    border-radius: 8px;
    min-width: 250px;
    text-align: center;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2);
  }

  .alert-box p {
    margin-bottom: 1rem;
  }

  .alert-box button {
    padding: 0.4rem 1.5rem;
  }

  .dialog-box {
    background: white;
    padding: 1.5rem;
    border-radius: 8px;
    min-width: 250px;
    text-align: center;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2);
  }

  .dialog-box p {
    margin-bottom: 1.5rem;
  }

  .dialog-buttons {
    display: flex;
    justify-content: center;
    gap: 1rem;
  }

  .btn-cancel {
    padding: 0.4rem 1.5rem;
    background: #eee;
    border: none;
    border-radius: 4px;
    cursor: pointer;
  }

  .btn-ok {
    padding: 0.4rem 1.5rem;
    background: #333;
    color: white;
    border: none;
    border-radius: 4px;
    cursor: pointer;
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
    border-top: 6px solid #555; /* 用 border 畫三角形箭頭 */
    pointer-events: none; /* 不攔截點擊事件 */
  }

  select {
    appearance: none;
    -webkit-appearance: none;
    padding: 0.25rem 2rem 0.25rem 0.5rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    background-color: white;
    gap: 1rem;
    cursor: pointer;
    font-size: 1rem;
  }

  select:hover {
    border-color: #888;
  }

  select:focus {
    outline: none;
    border-color: #333;
  }

  .progress-bar-bg {
    width: 100%;
    height: 10px;
    background: #eee;
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

  .fail-detail {
    color: #c0392b;
    font-size: 0.9rem;
  }
</style>
