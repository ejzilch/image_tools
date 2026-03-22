<script>
    import { shrinkMode, width, height, ratio } from "../stores/shrink.js";

    function onShrinkModeChange() {
        if ($shrinkMode === "dimension") {
            ratio.set("");
        } else {
            width.set("");
            height.set("");
        }
    }
</script>

<section>
    <p class="label">參數設定</p>

    <div class="radio-group">
        <label>
            <input
                type="radio"
                bind:group={$shrinkMode}
                value="dimension"
                on:change={onShrinkModeChange}
            /> 指定寬高
        </label>
        <label>
            <input
                type="radio"
                bind:group={$shrinkMode}
                value="ratio"
                on:change={onShrinkModeChange}
            /> 維持比例縮小
        </label>
    </div>

    {#if $shrinkMode === "dimension"}
        <div class="fields-row">
            <label
                >寬度　<input
                    type="number"
                    bind:value={$width}
                    min="1"
                    max="16383"
                />（px）</label
            >
            <label
                >高度　<input
                    type="number"
                    bind:value={$height}
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
                    bind:value={$ratio}
                    min="1"
                    max="99"
                />（%）</label
            >
        </div>
    {/if}
</section>

<style>
    .label {
        font-weight: bold;
        margin-bottom: 0.5rem;
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
</style>
