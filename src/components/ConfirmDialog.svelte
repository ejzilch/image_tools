<script>
    export let message = "";
    export let onOk = () => {};
    export let onCancel = () => {};
</script>

<div class="overlay">
    <div class="dialog-box">
        <p>{message}</p>
        <div class="dialog-buttons">
            <button class="btn-ok" onclick={onOk}>確定</button>
            <button class="btn-cancel" onclick={onCancel}>取消</button>
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
