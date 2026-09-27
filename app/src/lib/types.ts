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
  engine: "parakeet" | "whisper";
  description: string;
  size_bytes: number;
  min_ram_mb: number;
  state: ModelState;
  removable: boolean;
  compatible: boolean;
  recommended: boolean;
  license_id: string;
  attribution: string;
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
  hardware: { total_ram_mb: number; logical_cores: number; avx2: boolean };
  engine: EngineStatus;
  mic_consent: "allowed" | "blocked";
  autostart: boolean;
  data_dir: string;
  notice: string | null;
  last_transcript_available: boolean;
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
