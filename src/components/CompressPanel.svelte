<script>
    import { targetSize, targetUnit } from "../stores/compress.js";
</script>

<section>
    <p class="label">參數設定</p>

    <div class="field-row">
        <label for="target-size">目標大小</label>
        <input
            id="target-size"
            type="number"
            bind:value={$targetSize}
            min="1"
        />
        <div class="select-wrapper">
            <select bind:value={$targetUnit}>
                <option value="kb">KB</option>
                <option value="mb">MB</option>
            </select>
        </div>
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

    input[type="number"] {
        width: 80px;
        padding: 0.25rem 0.5rem;
        border: 1px solid var(--border);
        border-radius: 4px;
        background-color: var(--bg-secondary);
        color: var(--text);
        font-size: 1rem;
    }

    input[type="number"]:focus {
        outline: none;
        border-color: var(--border-focus);
    }

    input[type="number"]:hover {
        border-color: var(--border-hover);
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
</style>
