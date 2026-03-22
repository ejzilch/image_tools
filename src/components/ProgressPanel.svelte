<script>
    import {
        isProcessing,
        isDone,
        progress,
        successCount,
        failedFiles,
        progressPct,
    } from "../stores/progress.js";
    import { createEventDispatcher } from "svelte";

    const dispatch = createEventDispatcher();
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
            <button class="cancel" on:click={() => dispatch("cancel")}
                >取消</button
            >
        {/if}

        {#if $isDone}
            <p class="label">完成</p>
            <p class="success">✅ 成功：{$successCount} 個</p>
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
</style>
