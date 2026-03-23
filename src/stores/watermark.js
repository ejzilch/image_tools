import { writable } from "svelte/store";
import { load } from "@tauri-apps/plugin-store";

export const wmSettings = writable({
    text: "",
    fontName: "NotoSans",
    position: "bottom-right",
    opacity: 70,
    color: "#ffffff",
    fontSize: 40,
    bold: false,
});

load("settings.json").then(async (store) => {
    const saved = await store.get("watermark");
    if (saved) wmSettings.set(saved);
});

export async function saveWmSettings(data) {
    wmSettings.set(data);
    const store = await load("settings.json");
    await store.set("watermark", data);
    await store.save();
}