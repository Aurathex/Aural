// Mirrors the Rust types serialized by the Tauri commands (app/src-tauri/src/state.rs).

export type HotkeyMode = "push_to_talk" | "toggle";
export type PillPosition = "bottom" | "top";

export interface Settings {
  schema_version: number;
  hotkey: { keys: string[]; mode: HotkeyMode };
  audio: { device: string | null };
  stt: { active_model: string | null; active_variant: string | null };
  startup: { launch_at_login: boolean };
  ui: { pill_position: PillPosition; sounds: boolean };
}

export type ModelState =
  | { kind: "available" }
  | { kind: "downloading"; downloaded: number; total: number }
  | { kind: "installed" }
  | { kind: "active" };

export interface ModelStatus {
  id: string;
  name: string;
  engine: "parakeet" | "whisper" | "moonshine";
  description: string;
  size_bytes: number;
  min_ram_mb: number;
  state: ModelState;
  removable: boolean;
  compatible: boolean;
  recommended: boolean;
  license_id: string;
  attribution: string;
  /** Variant ids, `"<model>@<backend>`". */
  variants: string[];
  runtime: "onnx" | "ggml";
  precision: string;
}

export interface InputDevice {
  name: string;
  is_default: boolean;
}

export type EngineStatus =
  | { state: "no_model" }
  | { state: "loading"; model: string }
  | { state: "ready"; model: string; label: string; backend: string }
  | { state: "error"; model: string; message: string };

export interface AppState {
  version: string;
  settings: Settings;
  hotkey_display: string;
  models: ModelStatus[];
  devices: InputDevice[];
  hardware: Hardware;
  engine: EngineStatus;
  mic_consent: "allowed" | "blocked";
  autostart: boolean;
  data_dir: string;
  notice: string | null;
  last_transcript_available: boolean;
  results: VariantResult[];
  labels: Record<string, Label[]>;
  hardware_test: HwTestStatus;
}

export type Verdict =
  | { verdict: "ok" }
  | { verdict: "warn"; message: string }
  | { verdict: "reject"; message: string };

export interface HotkeyCheck {
  display: string;
  verdict: Verdict;
}

export type PillState =
  | { state: "hidden" }
  | { state: "listening" }
  | { state: "processing" }
  | { state: "success" }
  | { state: "error"; error: string };

export interface PillView {
  state: PillState;
  label: string | null;
}

export interface ProgressEvent {
  id: string;
  downloaded: number;
  total: number;
}

export type Backend = "cpu" | "vulkan" | "cuda" | "directml";

export interface Gpu {
  name: string;
  vendor: "nvidia" | "amd" | "intel" | "other";
  vram_mb: number;
  integrated: boolean;
  driver: string;
  luid: number;
}

export interface Hardware {
  cpu_name: string;
  total_ram_mb: number;
  free_ram_mb: number;
  logical_cores: number;
  physical_cores: number;
  avx2: boolean;
  avx512: boolean;
  gpus: Gpu[];
}

export interface RunMetrics {
  wer: number;
  words: number;
  p50_ms: number;
  p95_ms: number;
  rtf: number;
}

export type Stability = { state: "stable" } | { state: "unstable"; reason: string };

export interface VariantResult {
  variant: string;
  metrics: RunMetrics | null;
  load_ms: number;
  ram_mb: number;
  vram_mb: number | null;
  spread: number;
  passes: number;
  stability: Stability;
  measured: boolean;
  error: string | null;
}

export type Reason =
  | { kind: "no_gpu" }
  | { kind: "gpu_failed"; detail: string }
  | { kind: "not_enough_memory"; need_mb: number }
  | { kind: "not_enough_gpu_memory"; need_mb: number }
  | { kind: "too_slow" }
  | { kind: "too_many_mistakes" }
  | { kind: "unstable"; detail: string };

export type Label =
  | { label: "recommended" }
  | { label: "most_accurate" }
  | { label: "fastest" }
  | { label: "live_text" }
  | { label: "wont_work_well"; reason: Reason };

export type HwStep =
  | { step: "detecting" }
  | { step: "downloading_probes" }
  | { step: "calibrating"; backend: Backend }
  | { step: "measuring"; variant: string }
  | { step: "estimating" }
  | { step: "done" };

export interface HwTestStatus {
  running: boolean;
  step: HwStep | null;
  done: number;
  total: number;
  tested: boolean;
  stale: boolean;
  /** The two small test models are already on this PC. */
  test_models_installed: boolean;
}