// Demo backend used only when the UI runs in a plain browser (design review and
// screenshots). It never runs inside the app. Model data mirrors the real catalog.
// Scenarios: `?pc=gpu` (default: RTX 4070 laptop) or `?pc=cpu` (8 GB laptop, no
// graphics card); add `&fresh` for a PC that has never been checked.

import type {
  AppState,
  Backend,
  Hardware,
  HotkeyCheck,
  HwStep,
  Label,
  ModelStatus,
  Settings,
  VariantResult,
} from "./types";

type Handler = (payload: unknown) => void;
const handlers = new Map<string, Set<Handler>>();

function emit(event: string, payload: unknown) {
  handlers.get(event)?.forEach((h) => h(payload));
}

const params = typeof location === "undefined" ? new URLSearchParams() : new URLSearchParams(location.search);
const gpuPc = params.get("pc") !== "cpu";
const fresh = params.has("fresh");

const settings: Settings = {
  schema_version: 1,
  hotkey: { keys: ["Ctrl", "Win"], mode: "push_to_talk" },
  audio: { device: null },
  stt: { active_model: null, active_variant: null },
  startup: { launch_at_login: false },
  ui: { pill_position: "bottom", sounds: true },
};

function model(
  id: string,
  name: string,
  engine: ModelStatus["engine"],
  description: string,
  size_bytes: number,
  min_ram_mb: number,
  backends: Backend[],
  license_id: string,
  precision: string,
): ModelStatus {
  return {
    id,
    name,
    engine,
    description,
    size_bytes,
    min_ram_mb,
    state: { kind: "available" },
    removable: false,
    compatible: true,
    recommended: false,
    license_id,
    attribution: `${name} (${license_id}).`,
    variants: backends.map((b) => `${id}@${b}`),
    runtime: engine === "whisper" ? "ggml" : "onnx",
    precision,
  };
}

const models: ModelStatus[] = [
  model("parakeet-tdt-0.6b-v2-int8", "Parakeet TDT 0.6B v2", "parakeet", "Very accurate for English. Quick on most modern PCs.", 661_331_448, 2048, ["cpu", "directml"], "CC-BY-4.0", "int8"),
  model("parakeet-tdt-0.6b-v3-int8", "Parakeet TDT 0.6B v3", "parakeet", "Newer Parakeet that also understands 24 other European languages. Very accurate.", 670_619_706, 2048, ["cpu", "directml"], "CC-BY-4.0", "int8"),
  model("moonshine-base-int8", "Moonshine base", "moonshine", "Small and quick. A good fit for older or low-memory PCs.", 66_774_609, 512, ["cpu"], "MIT", "int8"),
  model("whisper-large-v3-turbo-q5", "Whisper large-v3 turbo", "whisper", "Whisper's large model, sped up. Very accurate; works best with a graphics card.", 574_041_195, 1536, ["cpu", "vulkan"], "MIT", "q5_0"),
  model("whisper-distil-large-v3.5", "Distil-Whisper large-v3.5", "whisper", "A faster, lighter version of Whisper's large model. Very accurate; works best with a graphics card.", 1_519_521_155, 3072, ["cpu", "vulkan"], "MIT", "fp16"),
  model("whisper-small.en-q8", "Whisper small.en", "whisper", "Good accuracy, lighter on memory. A solid choice for older PCs.", 264_477_561, 1024, ["cpu", "vulkan"], "MIT", "q8_0"),
  model("whisper-base.en-q8", "Whisper base.en", "whisper", "Smallest and quickest to download. Less accurate.", 81_781_811, 512, ["cpu", "vulkan"], "MIT", "q8_0"),
];

function result(variant: string, wer: number, p50: number, ram: number, vram: number | null, measured: boolean, load = 1500): VariantResult {
  return {
    variant,
    metrics: { wer, words: measured ? 396 : 0, p50_ms: p50, p95_ms: Math.round(p50 * 1.5), rtf: p50 / 7700 },
    load_ms: load,
    ram_mb: ram,
    vram_mb: vram,
    spread: 1.5,
    passes: measured ? 3 : 0,
    stability: { state: "stable" },
    measured,
    error: null,
  };
}

const R = "recommended", A = "most_accurate", F = "fastest";
const L = (...ls: string[]) => ls.map((label) => ({ label }) as Label);
const slow: Label[] = [{ label: "wont_work_well", reason: { kind: "too_slow" } }];
const noGpu: Label[] = [{ label: "wont_work_well", reason: { kind: "no_gpu" } }];

const hardware: Hardware = gpuPc
  ? {
      cpu_name: "Intel(R) Core(TM) Ultra 9 185H",
      total_ram_mb: 15_770,
      free_ram_mb: 6_200,
      logical_cores: 22,
      physical_cores: 16,
      avx2: true,
      avx512: false,
      gpus: [
        { name: "NVIDIA GeForce RTX 4070 Laptop GPU", vendor: "nvidia", vram_mb: 7_948, integrated: false, driver: "32.0.16.1714", luid: 1 },
        { name: "Intel(R) Arc(TM) Graphics", vendor: "intel", vram_mb: 2_048, integrated: true, driver: "32.0.101.8132", luid: 2 },
      ],
    }
  : {
      cpu_name: "Intel(R) Core(TM) i5-1135G7",
      total_ram_mb: 7_900,
      free_ram_mb: 3_600,
      logical_cores: 8,
      physical_cores: 4,
      avx2: true,
      avx512: true,
      gpus: [{ name: "Intel(R) Iris(R) Xe Graphics", vendor: "intel", vram_mb: 128, integrated: true, driver: "31.0.101.5186", luid: 1 }],
    };

const results: VariantResult[] = gpuPc
  ? [
      result("parakeet-tdt-0.6b-v2-int8@cpu", 0.005, 350, 772, null, true, 1858),
      result("parakeet-tdt-0.6b-v2-int8@directml", 0.005, 664, 769, 1001, true, 3021),
      result("parakeet-tdt-0.6b-v3-int8@cpu", 0.012, 320, 900, null, false),
      result("parakeet-tdt-0.6b-v3-int8@directml", 0.012, 610, 900, 1100, false),
      result("moonshine-base-int8@cpu", 0.023, 460, 250, null, false),
      result("whisper-large-v3-turbo-q5@cpu", 0.02, 15_200, 1004, null, false),
      result("whisper-large-v3-turbo-q5@vulkan", 0.02, 310, 620, 1200, false),
      result("whisper-distil-large-v3.5@cpu", 0.03, 14_500, 1885, null, false),
      result("whisper-distil-large-v3.5@vulkan", 0.03, 320, 1105, 1800, false),
      result("whisper-small.en-q8@cpu", 0.031, 2_700, 562, null, false),
      result("whisper-small.en-q8@vulkan", 0.031, 150, 400, 700, false),
      result("whisper-base.en-q8@cpu", 0.049, 800, 275, null, false),
      result("whisper-base.en-q8@vulkan", 0.049, 90, 300, 400, false),
    ]
  : [
      result("parakeet-tdt-0.6b-v2-int8@cpu", 0.005, 460, 772, null, true, 2600),
      result("parakeet-tdt-0.6b-v3-int8@cpu", 0.012, 430, 900, null, false),
      result("moonshine-base-int8@cpu", 0.023, 610, 250, null, false),
      result("whisper-large-v3-turbo-q5@cpu", 0.02, 21_000, 1004, null, false),
      result("whisper-distil-large-v3.5@cpu", 0.03, 20_000, 1885, null, false),
      result("whisper-small.en-q8@cpu", 0.031, 3_800, 562, null, false),
      result("whisper-base.en-q8@cpu", 0.049, 1_100, 275, null, false),
    ];

const labels: Record<string, Label[]> = gpuPc
  ? {
      "parakeet-tdt-0.6b-v2-int8@cpu": L(R, A, F),
      "parakeet-tdt-0.6b-v2-int8@directml": [],
      "parakeet-tdt-0.6b-v3-int8@cpu": [],
      "parakeet-tdt-0.6b-v3-int8@directml": [],
      "moonshine-base-int8@cpu": [],
      "whisper-large-v3-turbo-q5@cpu": slow,
      "whisper-large-v3-turbo-q5@vulkan": [],
      "whisper-distil-large-v3.5@cpu": slow,
      "whisper-distil-large-v3.5@vulkan": [],
      "whisper-small.en-q8@cpu": [],
      "whisper-small.en-q8@vulkan": [],
      "whisper-base.en-q8@cpu": [],
      "whisper-base.en-q8@vulkan": [],
    }
  : {
      "parakeet-tdt-0.6b-v2-int8@cpu": L(R, A, F),
      "parakeet-tdt-0.6b-v2-int8@directml": noGpu,
      "parakeet-tdt-0.6b-v3-int8@cpu": [],
      "parakeet-tdt-0.6b-v3-int8@directml": noGpu,
      "moonshine-base-int8@cpu": [],
      "whisper-large-v3-turbo-q5@cpu": slow,
      "whisper-large-v3-turbo-q5@vulkan": noGpu,
      "whisper-distil-large-v3.5@cpu": [{ label: "wont_work_well", reason: { kind: "not_enough_memory", need_mb: 3072 } }],
      "whisper-distil-large-v3.5@vulkan": noGpu,
      "whisper-small.en-q8@cpu": [],
      "whisper-small.en-q8@vulkan": noGpu,
      "whisper-base.en-q8@cpu": [],
      "whisper-base.en-q8@vulkan": noGpu,
    };

if (!fresh) {
  const pk = models[0]!;
  pk.state = { kind: "active" };
  settings.stt.active_model = pk.id;
  settings.stt.active_variant = `${pk.id}@cpu`;
}

const state: AppState = {
  version: "0.2.0",
  settings,
  hotkey_display: "Ctrl + Win",
  models,
  devices: [
    { name: "Microphone Array (Realtek Audio)", is_default: true },
    { name: "Headset Microphone (USB Audio)", is_default: false },
  ],
  hardware,
  engine: fresh
    ? { state: "no_model" }
    : { state: "ready", model: models[0]!.id, label: models[0]!.name, backend: "cpu" },
  mic_consent: "allowed",
  autostart: false,
  data_dir: "C:\\Users\\you\\AppData\\Local\\com.aurathex.aural",
  notice: null,
  last_transcript_available: false,
  results: fresh ? [] : results,
  labels: fresh ? {} : labels,
  hardware_test: { running: false, step: null, done: 0, total: 0, tested: !fresh, stale: false, test_models_installed: !fresh },
};

let levelTimer: number | undefined;
let testTimer: number | undefined;

function runTest(allowDownload: boolean) {
  const steps: HwStep[] = [
    { step: "detecting" },
    ...(allowDownload ? [{ step: "downloading_probes" } as HwStep] : []),
    { step: "calibrating", backend: "cpu" },
    ...(gpuPc ? [{ step: "calibrating", backend: "vulkan" } as HwStep] : []),
    { step: "measuring", variant: `${models[0]!.id}@cpu` },
    { step: "estimating" },
  ];
  let i = 0;
  state.hardware_test = { ...state.hardware_test, running: true, step: steps[0]!, done: 0, total: steps.length };
  testTimer = window.setInterval(() => {
    i++;
    if (i >= steps.length) {
      window.clearInterval(testTimer);
      state.results = results;
      state.labels = allowDownload ? labels : {};
      state.hardware_test = { running: false, step: { step: "done" }, done: 0, total: 0, tested: true, stale: false, test_models_installed: allowDownload };
      emit("state-changed", structuredClone(state));
      return;
    }
    state.hardware_test = { ...state.hardware_test, step: steps[i]!, done: i };
    emit("hwtest-progress", structuredClone(state.hardware_test));
  }, 900);
}

export const demo = {
  on(event: string, handler: Handler): () => void {
    if (!handlers.has(event)) handlers.set(event, new Set());
    handlers.get(event)!.add(handler);
    return () => handlers.get(event)?.delete(handler);
  },

  async call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    switch (cmd) {
      case "get_state":
      case "dismiss_notice":
        return structuredClone(state) as T;
      case "check_hotkey": {
        const keys = (args?.keys as string[]) ?? [];
        const display = keys.join(" + ");
        const r: HotkeyCheck =
          keys.length === 1 && ["Ctrl", "Alt", "Shift", "Win"].includes(keys[0]!)
            ? { display, verdict: { verdict: "reject", message: "A single modifier is used by too many shortcuts." } }
            : { display, verdict: { verdict: "ok" } };
        return r as T;
      }
      case "save_settings": {
        Object.assign(state.settings, args?.settings as Settings);
        state.hotkey_display = state.settings.hotkey.keys.join(" + ");
        state.autostart = state.settings.startup.launch_at_login;
        return structuredClone(state) as T;
      }
      case "start_hardware_test":
        runTest(Boolean(args?.allowProbeDownload));
        return structuredClone(state) as T;
      case "cancel_hardware_test":
        window.clearInterval(testTimer);
        state.hardware_test = { ...state.hardware_test, running: false, step: null };
        emit("state-changed", structuredClone(state));
        return undefined as T;
      case "download_model": {
        const m = state.models.find((x) => x.id === args?.id);
        if (m) {
          let done = 0;
          m.state = { kind: "downloading", downloaded: 0, total: m.size_bytes };
          const t = window.setInterval(() => {
            done = Math.min(m.size_bytes, done + m.size_bytes / 25);
            emit("model-progress", { id: m.id, downloaded: done, total: m.size_bytes });
            if (done >= m.size_bytes) {
              window.clearInterval(t);
              m.state = state.settings.stt.active_model ? { kind: "installed" } : { kind: "active" };
              m.removable = m.state.kind === "installed";
              if (!state.settings.stt.active_model) {
                state.settings.stt.active_model = m.id;
                state.settings.stt.active_variant = `${m.id}@cpu`;
                state.engine = { state: "ready", model: m.id, label: m.name, backend: "cpu" };
              }
              emit("state-changed", structuredClone(state));
            }
          }, 120);
        }
        return structuredClone(state) as T;
      }
      case "use_variant": {
        const variant = args?.id as string;
        const id = variant.split("@")[0] ?? variant;
        for (const m of state.models) {
          if (m.state.kind === "active") { m.state = { kind: "installed" }; m.removable = true; }
          if (m.id === id) { m.state = { kind: "active" }; m.removable = false; }
        }
        state.settings.stt.active_model = id;
        state.settings.stt.active_variant = variant;
        return structuredClone(state) as T;
      }
      case "remove_model": {
        const m = state.models.find((x) => x.id === args?.id);
        if (m) { m.state = { kind: "available" }; m.removable = false; }
        return structuredClone(state) as T;
      }
      case "mic_test_start":
        levelTimer = window.setInterval(() => {
          const t = performance.now() / 300;
          emit("mic-levels", Array.from({ length: 12 }, (_, i) => Math.max(0, 0.55 + 0.4 * Math.sin(t + i * 0.7) - i * 0.03)));
        }, 33);
        return undefined as T;
      case "mic_test_stop":
        window.clearInterval(levelTimer);
        return undefined as T;
      default:
        return undefined as T;
    }
  },
};
