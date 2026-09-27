// Demo backend used only when the UI runs in a plain browser (design review and
// screenshots). It never runs inside the app. Model data mirrors the real catalog.

import type { AppState, HotkeyCheck, Settings } from "./types";

type Handler = (payload: unknown) => void;
const handlers = new Map<string, Set<Handler>>();

function emit(event: string, payload: unknown) {
  handlers.get(event)?.forEach((h) => h(payload));
}

const settings: Settings = {
  schema_version: 1,
  hotkey: { keys: ["Ctrl", "Win"], mode: "push_to_talk" },
  audio: { device: null },
  stt: { active_model: null, active_variant: null },
  startup: { launch_at_login: false },
  ui: { pill_position: "bottom", sounds: true },
};

const state: AppState = {
  version: "0.1.0",
  settings,
  hotkey_display: "Ctrl + Win",
  models: [
    {
      id: "parakeet-tdt-0.6b-v2-int8",
      name: "Parakeet TDT 0.6B v2",
      engine: "parakeet",
      description: "Most accurate English model. Fast on modern CPUs.",
      size_bytes: 661_331_448,
      min_ram_mb: 2048,
      state: { kind: "available" },
      removable: false,
      compatible: true,
      recommended: true,
      license_id: "CC-BY-4.0",
      attribution: "Parakeet TDT 0.6B v2 by NVIDIA, licensed CC BY 4.0.",
    },
    {
      id: "whisper-small.en-q8",
      name: "Whisper small.en",
      engine: "whisper",
      description: "Good accuracy, lighter on memory. A solid choice for older PCs.",
      size_bytes: 264_477_561,
      min_ram_mb: 1024,
      state: { kind: "available" },
      removable: false,
      compatible: true,
      recommended: false,
      license_id: "MIT",
      attribution: "Whisper small.en by OpenAI (MIT).",
    },
    {
      id: "whisper-base.en-q8",
      name: "Whisper base.en",
      engine: "whisper",
      description: "Smallest and quickest to download. Less accurate.",
      size_bytes: 81_781_811,
      min_ram_mb: 512,
      state: { kind: "available" },
      removable: false,
      compatible: true,
      recommended: false,
      license_id: "MIT",
      attribution: "Whisper base.en by OpenAI (MIT).",
    },
  ],
  devices: [
    { name: "Microphone Array (Realtek Audio)", is_default: true },
    { name: "Headset Microphone (USB Audio)", is_default: false },
  ],
  hardware: { total_ram_mb: 15_770, logical_cores: 22, avx2: true },
  engine: { state: "no_model" },
  mic_consent: "allowed",
  autostart: false,
  data_dir: "C:\\Users\\you\\AppData\\Local\\Aural",
  notice: null,
  last_transcript_available: false,
};

let levelTimer: number | undefined;

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
        const model = variant.split("@")[0] ?? variant;
        for (const m of state.models) {
          if (m.state.kind === "active") { m.state = { kind: "installed" }; m.removable = true; }
          if (m.id === model) { m.state = { kind: "active" }; m.removable = false; }
        }
        state.settings.stt.active_model = model;
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
