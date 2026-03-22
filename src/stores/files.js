import { writable } from "svelte/store";

export const selectedFiles = writable([]);

export function addFiles(newPaths) {
    selectedFiles.update((current) => {
        const deduplicated = newPaths.filter((p) => !current.includes(p));
        return [...current, ...deduplicated];
    });
}

export function removeFile(index) {
    selectedFiles.update((current) => current.filter((_, i) => i !== index));
}

export function clearFiles() {
    selectedFiles.set([]);
}