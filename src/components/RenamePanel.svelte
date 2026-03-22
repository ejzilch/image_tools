<script>
    import { renameText, renameMode } from "../stores/rename.js";

    $: dateStr = new Date().toISOString().slice(0, 10);

    $: preview =
        $renameMode === "date_sequence"
            ? `${$renameText || "photo"}_${dateStr}_00001.jpg`
            : `${$renameText || "photo"}_00001.jpg`;
</script>

<section>
    <p class="label">參數設定</p>

    <div class="field-row">
        <label for="rename-text">自訂文字</label>
        <input
            id="rename-text"
            type="text"
            bind:value={$renameText}
            placeholder="photo"
        />
    </div>

    <div class="rename-options">
        <label>
            <input type="radio" bind:group={$renameMode} value="sequence" />
            流水號
        </label>
        <label>
            <input
                type="radio"
                bind:group={$renameMode}
                value="date_sequence"
            />
            日期＋流水號
        </label>
        <p class="rename-preview">{preview}</p>
    </div>
</section>

<style>
    .label {
        font-weight: bold;
        margin-bottom: 0.5rem;
    }

    .field-row {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        margin-top: 0.75rem;
    }

    .field-row input[type="text"] {
        padding: 0.25rem 0.5rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background-color: var(--bg-secondary);
        color: var(--text);
        font-size: 1rem;
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
        width: 100%;
        font-size: 0.9rem;
        color: var(--text-muted);
        font-family: monospace;
        margin: 0;
        padding: 0;
        text-align: left;
    }
</style>
