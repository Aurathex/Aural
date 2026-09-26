import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { demo } from "./demo";

/** True inside the Aural app; false when the UI is opened in a plain browser. */
export const inApp = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return inApp ? invoke<T>(cmd, args) : demo.call<T>(cmd, args);
}

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (inApp) return listen<T>(event, (e) => handler(e.payload));
  return Promise.resolve(demo.on(event, handler as (p: unknown) => void));
}
