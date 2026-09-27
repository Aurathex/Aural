// Everyday wording for the Models page and the Hardware Test (spec §5.1). Technical
// terms appear only inside "Technical details"; the tests scan for them.

import type { Backend, Hardware, HwStep, Label, Reason } from "./types";

/** Words a non-technical person shouldn't meet outside "Technical details". */
export const JARGON: readonly string[] = [
  "WER",
  "RTF",
  "VRAM",
  "DirectML",
  "Vulkan",
  "ONNX",
  "ggml",
  "int8",
  "fp16",
  "q5_0",
  "backend",
  "inference",
];

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
// Names written in capitals or mixed case only count in that spelling (so a code value
// like "directml" isn't flagged); lowercase terms count in any case.
const JARGON_RE = JARGON.map(
  (j) => new RegExp(`(^|[^A-Za-z0-9_])${escape(j)}($|[^A-Za-z0-9_])`, j === j.toLowerCase() ? "i" : ""),
);

export function hasJargon(s: string): boolean {
  return JARGON_RE.some((re) => re.test(s));
}

/** Removes `{…}` expressions, including nested braces. */
function stripBraces(s: string): string {
  let out = "";
  let depth = 0;
  for (const ch of s) {
    if (ch === "{") depth++;
    else if (ch === "}") depth = Math.max(0, depth - 1);
    else if (depth === 0) out += ch;
  }
  return out;
}

/**
 * The text a person can see or hear in a Svelte component: markup text, the
 * accessible attributes (aria-label, title, placeholder, alt) and string literals in
 * its script. Code, comments, styles and `<details class="tech">` are left out.
 */
export function userFacingText(src: string): string[] {
  const out: string[] = [];
  let rest = src.replace(/<style[\s\S]*?<\/style>/g, "");
  rest = rest.replace(/<script[\s\S]*?>([\s\S]*?)<\/script>/g, (_m, code: string) => {
    const noComments = code.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`])\/\/.*$/gm, "$1");
    for (const m of noComments.matchAll(/"((?:[^"\\\n]|\\.)*)"|'((?:[^'\\\n]|\\.)*)'|`((?:[^`\\]|\\.)*)`/g)) {
      const s = (m[1] ?? m[2] ?? m[3] ?? "").replace(/\$\{[^}]*\}/g, " ");
      if (s.trim()) out.push(s.trim());
    }
    return "";
  });
  rest = rest.replace(/<!--[\s\S]*?-->/g, "");
  rest = rest.replace(/<details\s+class="tech"[\s\S]*?<\/details>/g, "");
  for (const m of rest.matchAll(/\s(?:aria-label|title|placeholder|alt)="([^"]*)"/g)) {
    const s = stripBraces(m[1] ?? "").trim();
    if (s) out.push(s);
  }
  const text = stripBraces(rest).replace(/<[^>]*>/g, "\n");
  for (const line of text.split("\n")) {
    const s = line.trim();
    if (s) out.push(s);
  }
  return out;
}

const gb = (mb: number) => mb / 1024;

export function accuracyText(wer: number): string {
  const right = Math.floor((1 - wer) * 100 + 1e-9);
  return `About ${Math.max(0, right)} in 100 words right`;
}

export function speedText(p50Ms: number): string {
  if (p50Ms < 100) return "Text appears almost instantly after you stop talking";
  const s = p50Ms / 1000;
  const shown = s >= 10 ? String(Math.round(s)) : s.toFixed(1);
  return `Text appears about ${shown} s after you stop talking`;
}

export function startupText(loadMs: number): string {
  const s = Math.max(1, Math.round(loadMs / 1000));
  return `Ready about ${s} s after you switch to it`;
}

function amount(mb: number): string {
  return gb(mb) < 0.1 ? "less than 0.1 GB" : `about ${gb(mb).toFixed(1)} GB`;
}

export function memoryText(ramMb: number, vramMb?: number | null): string {
  const pc = `Uses ${amount(ramMb)} of your PC's memory`;
  return vramMb ? `${pc} and ${amount(vramMb)} of your graphics card's memory` : pc;
}

export function runsOnText(backend: Backend): string {
  return backend === "cpu" ? "Runs on your processor" : "Runs on your graphics card";
}

export function placeText(backend: Backend): string {
  return backend === "cpu" ? "On your processor" : "On your graphics card";
}

export function measuredText(measured: boolean): string {
  return measured ? "Measured on your PC" : "Expected, based on a quick test of your PC";
}

const wholeGb = (mb: number) => Math.max(1, Math.round(gb(mb)));
const capitalize = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

export function reasonText(r: Reason): string {
  switch (r.kind) {
    case "no_gpu":
      return "Needs a separate graphics card, and this PC doesn't have one";
    case "gpu_failed":
      return "Didn't work on this PC's graphics card";
    case "not_enough_memory":
      return `Needs about ${wholeGb(r.need_mb)} GB of free memory`;
    case "not_enough_gpu_memory":
      return `Needs a graphics card with at least ${wholeGb(r.need_mb)} GB of memory`;
    case "too_slow":
      return "Too slow on this PC to keep up with speech";
    case "too_many_mistakes":
      return "Makes too many mistakes on this PC";
    case "unstable":
      return capitalize(r.detail);
  }
}

export function labelText(l: Label): { name: string; hint: string } {
  switch (l.label) {
    case "recommended":
      return { name: "Recommended", hint: "The best balance of accuracy and speed for this PC" };
    case "most_accurate":
      return { name: "Most accurate", hint: "Makes the fewest mistakes" };
    case "fastest":
      return { name: "Fastest", hint: "Text appears soonest" };
    case "live_text":
      return { name: "Live text", hint: "Shows words while you speak" };
    case "wont_work_well":
      return { name: "Won't work well here", hint: reasonText(l.reason) };
  }
}

const cleanName = (s: string) => s.replace(/\((R|TM|C)\)/g, "").replace(/\s+/g, " ").trim();

export function hardwareText(hw: Hardware): string {
  const cards = hw.gpus.filter((g) => !g.integrated);
  const gpu = cards.length
    ? `Graphics card: ${cards.map((g) => `${cleanName(g.name)} (${wholeGb(g.vram_mb)} GB)`).join(", ")}`
    : "no separate graphics card";
  const cpu = hw.cpu_name ? `${cleanName(hw.cpu_name)}, ` : "";
  return `Your PC: ${cpu}${Math.round(gb(hw.total_ram_mb))} GB of memory · ${gpu}`;
}

export function variantBackend(variant: string): Backend {
  return (variant.split("@")[1] ?? "cpu") as Backend;
}

export function stepText(s: HwStep, modelName?: string): string {
  switch (s.step) {
    case "detecting":
      return "Checking your processor, memory and graphics card…";
    case "downloading_probes":
      return "Downloading two small test models…";
    case "calibrating":
      return s.backend === "cpu" ? "Timing a small test model on your processor…" : "Checking your graphics card…";
    case "measuring": {
      const where = variantBackend(s.variant) === "cpu" ? "your processor" : "your graphics card";
      return `Trying ${modelName ?? "a model"} on ${where}…`;
    }
    case "estimating":
      return "Working out which models suit your PC…";
    case "done":
      return "Done";
  }
}
