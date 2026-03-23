<script>
    import { selectedFiles, removeFile, clearFiles } from "../stores/files.js";
    let { onClose } = $props();
</script>

<div class="overlay">
    <div class="dialog-box file-list-dialog">
        <p class="label">已選擇的檔案（共 {$selectedFiles.length} 個）</p>
        <ul class="file-list-modal">
            {#each $selectedFiles as f, i}
                <li>
                    <span title={f}>{f.split("\\").pop().split("/").pop()}</span
                    >
                    <button class="remove-btn" onclick={() => removeFile(i)}
                        >✕</button
                    >
                </li>
            {/each}
        </ul>
        <div class="dialog-buttons" style="margin-top: 1rem;">
            <button class="btn-ok" onclick={onClose}>關閉</button>
            <button
                class="btn-cancel"
                onclick={() => {
                    clearFiles();
                    onClose();
                }}>清除全部</button
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

    .file-list-dialog {
        width: 420px;
        text-align: left;
    }

    .label {
        font-weight: bold;
        margin-bottom: 0.5rem;
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
</style>
