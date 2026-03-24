import { writable, derived } from "svelte/store";

export const isProcessing = writable(false);
export const isDone = writable(false);
export const progress = writable({ current: 0, total: 0 });
export const successCount = writable(0);
export const failedFiles = writable([]);
export const outputDirs = writable([]);

export const progressPct = derived(
    progress,
    ($progress) =>
        $progress.total > 0 ? ($progress.current / $progress.total) * 100 : 0,
);