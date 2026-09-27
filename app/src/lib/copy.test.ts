import { describe, expect, it } from "vitest";
import {
  accuracyText,
  hasJargon,
  hardwareText,
  JARGON,
  labelText,
  measuredText,
  memoryText,
  reasonText,
  runsOnText,
  speedText,
  startupText,
  stepText,
  userFacingText,
} from "./copy";
import type { Label, Reason } from "./types";

const ALL_REASONS: Reason[] = [
  { kind: "no_gpu" },
  { kind: "gpu_failed", detail: "DirectML EP failed to initialise" },
  { kind: "not_enough_memory", need_mb: 3072 },
  { kind: "not_enough_gpu_memory", need_mb: 6000 },
  { kind: "too_slow" },
  { kind: "too_many_mistakes" },
  { kind: "unstable", detail: "stopped working during the test" },
];
const ALL_LABELS: Label[] = [
  { label: "recommended" },
  { label: "most_accurate" },
  { label: "fastest" },
  { label: "live_text" },
  ...ALL_REASONS.map((reason) => ({ label: "wont_work_well" as const, reason })),
];

describe("plain-language copy", () => {
  it("turns WER into words right out of 100", () => {
    expect(accuracyText(0.025)).toBe("About 97 in 100 words right");
    expect(accuracyText(0.005)).toBe("About 99 in 100 words right");
    expect(accuracyText(0)).toBe("About 100 in 100 words right");
  });

  it("rounds speed to tenths of a second", () => {
    expect(speedText(257)).toBe("Text appears about 0.3 s after you stop talking");
    expect(speedText(47)).toBe("Text appears almost instantly after you stop talking");
    expect(speedText(15_247)).toBe("Text appears about 15 s after you stop talking");
  });

  it("says how long a model takes to get ready", () => {
    expect(startupText(1858)).toBe("Ready about 2 s after you switch to it");
  });

  it("describes memory in GB of the PC and of the graphics card", () => {
    expect(memoryText(861)).toBe("Uses about 0.8 GB of your PC's memory");
    expect(memoryText(770, 1126)).toBe(
      "Uses about 0.8 GB of your PC's memory and about 1.1 GB of your graphics card's memory",
    );
    expect(memoryText(40)).toBe("Uses less than 0.1 GB of your PC's memory");
  });

  it("names where a model runs without jargon", () => {
    expect(runsOnText("cpu")).toBe("Runs on your processor");
    expect(runsOnText("directml")).toBe("Runs on your graphics card");
    expect(runsOnText("vulkan")).toBe("Runs on your graphics card");
  });

  it("says whether numbers were measured or expected", () => {
    expect(measuredText(true)).toBe("Measured on your PC");
    expect(measuredText(false)).toBe("Expected, based on a quick test of your PC");
  });

  it("explains every label in one plain line", () => {
    for (const l of ALL_LABELS) {
      const t = labelText(l);
      expect(t.name.length).toBeGreaterThan(3);
      expect(t.hint.length).toBeGreaterThan(10);
      expect(hasJargon(t.name + " " + t.hint), t.name + t.hint).toBe(false);
    }
    expect(labelText({ label: "live_text" }).name).toBe("Live text");
    expect(labelText({ label: "wont_work_well", reason: { kind: "too_slow" } }).name).toBe("Won't work well here");
  });

  it("gives reasons as advice, with sizes rounded to whole GB", () => {
    expect(reasonText({ kind: "not_enough_gpu_memory", need_mb: 6000 })).toBe(
      "Needs a graphics card with at least 6 GB of memory",
    );
    // A driver's technical message never reaches the plain view.
    expect(reasonText({ kind: "gpu_failed", detail: "DirectML EP failed" })).not.toContain("DirectML");
    for (const r of ALL_REASONS) expect(hasJargon(reasonText(r)), reasonText(r)).toBe(false);
  });

  it("describes the hardware and each test step in everyday words", () => {
    const hw = {
      cpu_name: "Intel(R) Core(TM) Ultra 9 185H",
      total_ram_mb: 15_770,
      free_ram_mb: 6_000,
      logical_cores: 22,
      physical_cores: 16,
      avx2: true,
      avx512: false,
      gpus: [
        { name: "Intel(R) Arc(TM) Graphics", vendor: "intel" as const, vram_mb: 2048, integrated: true, driver: "1", luid: 1 },
        { name: "NVIDIA GeForce RTX 4070 Laptop GPU", vendor: "nvidia" as const, vram_mb: 7948, integrated: false, driver: "2", luid: 2 },
      ],
    };
    const t = hardwareText(hw);
    expect(t).toContain("15 GB of memory");
    expect(t).toContain("NVIDIA GeForce RTX 4070 Laptop GPU (8 GB)");
    expect(hasJargon(t)).toBe(false);
    expect(hardwareText({ ...hw, gpus: [] })).toContain("no separate graphics card");
    for (const s of [
      stepText({ step: "detecting" }),
      stepText({ step: "downloading_probes" }),
      stepText({ step: "calibrating", backend: "vulkan" }),
      stepText({ step: "calibrating", backend: "cpu" }),
      stepText({ step: "measuring", variant: "parakeet-tdt-0.6b-v2-int8@directml" }, "Parakeet TDT 0.6B v2"),
      stepText({ step: "estimating" }),
      stepText({ step: "done" }),
    ]) {
      expect(s.length).toBeGreaterThan(3);
      expect(hasJargon(s), s).toBe(false);
    }
  });

  it("flags jargon as whole words only", () => {
    expect(hasJargon("Uses the DirectML backend")).toBe(true);
    expect(hasJargon("q5_0 weights")).toBe(true);
    expect(hasJargon("Runs on your graphics card")).toBe(false);
    expect(hasJargon("Answered")).toBe(false); // contains "wer" inside a word
    for (const j of JARGON) expect(hasJargon(`x ${j} y`), j).toBe(true);
  });

  it("every user-facing string outside Technical details is jargon-free", () => {
    const sources = import.meta.glob("../**/*.svelte", { query: "?raw", import: "default", eager: true }) as Record<
      string,
      string
    >;
    expect(Object.keys(sources).length).toBeGreaterThan(5);
    for (const [file, src] of Object.entries(sources)) {
      for (const s of userFacingText(src)) expect(hasJargon(s), `${file}: ${s}`).toBe(false);
    }
  });

  it("the scan ignores code, comments and the Technical details section", () => {
    const src = `<script lang="ts">
  // Talk to the backend
  const b = v.backend;
  const shown = "Runs on your graphics card";
</script>
<p>{result.backend} Hello</p>
<!-- ggml note -->
<details class="tech"><summary>Technical details</summary>WER 2.5% · Vulkan · int8</details>
<style>.x { color: red; }</style>`;
    const text = userFacingText(src);
    expect(text).toContain("Runs on your graphics card");
    expect(text.join(" ")).toContain("Hello");
    for (const s of text) expect(hasJargon(s), s).toBe(false);
    // …but it does catch jargon in visible text.
    expect(userFacingText("<p>Uses Vulkan</p>").some(hasJargon)).toBe(true);
    expect(userFacingText(`<script>const m = "fp16 model";</script>`).some(hasJargon)).toBe(true);
  });
});
