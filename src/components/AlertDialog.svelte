<script>
    export let message = "";
    export let onClose = () => {};
</script>

<div class="overlay">
    <div class="dialog-box">
        <p>{message}</p>
        <button class="btn-ok" onclick={onClose}>確定</button>
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

    .btn-ok {
        padding: 0.4rem 1.5rem;
        background: var(--btn-primary-bg);
        color: var(--btn-primary-text);
        border: none;
        border-radius: 4px;
        cursor: pointer;
    }
</style>
