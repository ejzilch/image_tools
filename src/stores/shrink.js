import { writable } from "svelte/store";

export const shrinkMode = writable("dimension");
export const width = writable("");
export const height = writable("");
export const ratio = writable("");