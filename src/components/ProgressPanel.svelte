<script>
    import {
        isProcessing,
        isDone,
        progress,
        successCount,
        failedFiles,
        progressPct,
        outputDirs,
    } from "../stores/progress.js";
    let { onCancel } = $props();
    import { invoke } from "@tauri-apps/api/core";

    function shortenPath(path) {
        const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
        if (parts.length <= 2) return path;
        return "📁 ..\\" + parts.slice(-2).join("\\");
    }
</script>

{#if $isProcessing || $isDone}
    <section class="results">
        {#if $isProcessing}
            <p class="label">
                {$progress.total === 0
                    ? "準備中..."
                    : `處理中... ${$progress.current} / ${$progress.total}`}
            </p>
            <div class="progress-bar-bg">
                <div
                    class="progress-bar-fill"
                    style="width: {$progressPct}%"
                ></div>
            </div>
            <button class="cancel" onclick={onCancel}>取消</button>
        {/if}

        {#if $isDone}
            <p class="label">完成</p>
            <p class="success">✅ 成功：{$successCount} 個</p>
            <p class="label">輸出位置：</p>
            <div class="dirs-list">
                {#each $outputDirs as dir}
                    <div class="dir-row">
                        <span
                            class="dir-path"
                            title={dir}
                            onclick={() => invoke("open_folder", { path: dir })}
                            role="button"
                            tabindex="0"
                            onkeydown={(e) =>
                                e.key === "Enter" &&
                                invoke("open_folder", { path: dir })}
                        >
                            {shortenPath(dir)}
                        </span>
                        <button
                            class="open-btn"
                            onclick={() => invoke("open_folder", { path: dir })}
                        >
                            開啟
                        </button>
                    </div>
                {/each}
            </div>
            {#if $failedFiles.length > 0}
                <p class="fail">❌ 失敗：{$failedFiles.length} 個</p>
                <div class="fail-list">
                    {#each $failedFiles as f}
                        <p class="fail-detail">{f.file}：{f.error}</p>
                    {/each}
                </div>
            {/if}
        {/if}
    </section>
{/if}

<style>
    .results {
        margin-top: 1rem;
        text-align: left;
    }

    .label {
        font-weight: bold;
        margin-bottom: 0.5rem;
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

    .success {
        color: #4caf50;
    }

    .fail {
        color: #e53935;
    }

    .fail-list {
        max-height: 120px;
        overflow-y: auto;
    }

    .fail-detail {
        color: #c0392b;
        font-size: 0.9rem;
    }

    .cancel {
        width: 100%;
        padding: 0.75rem;
        font-size: 1rem;
        margin-top: 0.5rem;
        background: #e53935;
        color: white;
        border: none;
        border-radius: 4px;
        cursor: pointer;
    }

    .dir-path {
        font-size: 1rem;
        padding: 0.2 0.6rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--bg-secondary);
        color: var(--text);
        cursor: pointer;
    }

    .open-btn {
        font-size: 1rem;
        padding: 0.2 0.6rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background: var(--bg-secondary);
        color: var(--text);
        cursor: pointer;
    }

    .dirs-list {
        max-height: 120px;
        overflow-y: auto;
        margin-bottom: 0.5rem;
    }
</style>
