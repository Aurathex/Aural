// Dev-only: every pill state side by side on light and dark backgrounds, with
// synthetic levels for the listening state. Open /pill-preview.html under `npm run dev`.
import { mount } from "svelte";
import Pill from "./Pill.svelte";
import type { PillView } from "../lib/types";

const words = { stable: "He hoped there would be stew for dinner, turnips and carrots and", tentative: "bruised potatoes" };
const states: [string, PillView][] = [
  ["Live text", { state: { state: "listening" }, label: null, caption: "bottom" }],
  ["Live text (pill at top)", { state: { state: "listening" }, label: null, caption: "top" }],
  ["Listening", { state: { state: "listening" }, label: null }],
  ["Processing", { state: { state: "processing" }, label: null }],
  ["Success", { state: { state: "success" }, label: null }],
  ["Error — mic", { state: { state: "error", error: "mic_blocked" }, label: "Mic blocked" }],
  ["Error — copied", { state: { state: "error", error: "insert_blocked" }, label: "Copied — Ctrl+V" }],
];

const root = document.getElementById("preview")!;
root.style.cssText = "display:grid;grid-template-columns:repeat(2,1fr);font:12px 'Segoe UI',sans-serif";
for (const bg of ["#ffffff", "#1f1f1f"]) {
  const col = document.createElement("div");
  col.style.cssText = `background:${bg};padding:24px;display:flex;flex-direction:column;gap:18px`;
  root.appendChild(col);
  for (const [name, view] of states) {
    const row = document.createElement("div");
    row.style.cssText = `display:flex;align-items:center;gap:16px;color:${bg === "#ffffff" ? "#555" : "#aaa"}`;
    const label = document.createElement("span");
    label.textContent = name;
    label.style.width = "110px";
    const slot = document.createElement("div");
    row.append(label, slot);
    col.appendChild(row);
    const t0 = performance.now();
    const levels = Array.from({ length: 12 }, (_, i) => 0.7 - i * 0.05);
    const props = $state({ preview: view, levels, words: view.caption ? words : null });
    mount(Pill, { target: slot, props });
    if (view.state.state === "listening") {
      setInterval(() => {
        const t = (performance.now() - t0) / 250;
        props.levels = Array.from({ length: 12 }, (_, i) => Math.max(0, 0.6 + 0.35 * Math.sin(t + i) - i * 0.04));
      }, 33);
    }
  }
}
