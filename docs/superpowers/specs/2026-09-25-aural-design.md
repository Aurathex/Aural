# Aural — Local-First Windows Dictation: Design Spec + Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Every implementation task follows superpowers:test-driven-development; every bug goes through superpowers:systematic-debugging; no task is marked done without superpowers:verification-before-completion; each milestone ends with superpowers:requesting-code-review; feature work happens in worktrees via superpowers:using-git-worktrees.

**Goal:** Open-source Windows 10/11 app: hold a global hotkey → speak → release → local STT → text lands in the focused app, with a small black/white listening pill.

**Architecture:** Tauri v2 app (Rust core + small Svelte web UI). The core owns the hotkey hook, mic capture, session state machine, text insertion and model manager. STT runs in **separate worker processes** (one for ONNX engines, one for whisper.cpp/ggml engines) that talk to the core over framed stdin/stdout IPC. Models and GPU runtime packs are downloaded on demand from Settings, never bundled.

**Tech Stack:** Rust (stable), Tauri v2, Svelte 5 + TypeScript + Vite, `windows` crate (Win32), `cpal` (WASAPI), `rubato`, `realfft`, `transcribe-rs`/ONNX Runtime (Parakeet), `whisper-rs`/whisper.cpp (ggml: CPU/Vulkan/CUDA), `reqwest`, `sha2`, `serde`, `tracing`, NSIS installer.

**Working name:** **Aural** (placeholder; rename before first public release — see Open Decisions).

**Spec:** this document, Part A. After approval it gets copied to `docs/superpowers/specs/2026-09-25-aural-design.md` and Part B to `docs/superpowers/plans/2026-09-25-aural-m0-m1.md` inside the new repo.

---

## Context

You want a personal, open-source, local-first stand-in for Wispr Flow's core loop on Windows: no account, no cloud audio, and hardware-appropriate local STT. This is a planning pass only. Nothing has been implemented. The working directory is an empty scratch folder with no git repo.

**Environment facts gathered:**
- Dev PC: Intel Core Ultra 9 185H, Intel Arc iGPU, 15.4 GB RAM, Windows 11. Rust/cargo, Node, .NET, git and gh are installed.
- You have an **RTX 4070 8 GB PC** for NVIDIA validation. You have **no AMD machine**.
- **v1 language scope: English only** (your answer).
- Intel Arc is **not** a primary v1 GPU target (your instruction).

**Prior art you should know about:** **Handy** (cjpais/Handy, MIT, Tauri) already does roughly 70% of this: Tauri, Parakeet + whisper.cpp through `transcribe-rs`, Vulkan on Windows, and clipboard paste. OpenWhispr (Electron, MIT) is similar. You asked for an original from-scratch project, so this plan does **not** fork Handy.
- We will read their public docs and approach and use their MIT *crates* (`transcribe-rs`) as dependencies.
- We will not copy their app code. If we ever copy an MIT snippet, it gets attribution.
- Forking Handy is the "fast path" alternative if you'd rather ship sooner (see Open Decisions).

---

# PART A — DESIGN SPEC

## 1. Product architecture

```
┌──────────────────────────── aural.exe (Tauri v2, Rust) ────────────────────────────┐
│ Hotkey thread (WH_KEYBOARD_LL) ─events─►  SessionMachine (pure, aural-core)        │
│ Audio thread (cpal/WASAPI) ─pcm/levels─►     │ effects                              │
│                                              ▼                                      │
│  Orchestrator ── StartCapture / Transcribe / Insert / ShowPill / CopyToClipboard    │
│      │                 │                         │                                  │
│      │           SttClient (IPC) ──────┐   Inserter (strategy chain, Win32)         │
│      │                                 │                                            │
│  ModelManager (manifest, download,     │   Settings store (JSON, versioned)         │
│  sha256, install/remove)               │   HwDetect (DXGI, CPU features, RAM)       │
│      │                                 │                                            │
│  WebView windows: [pill] (transparent, no-activate)   [settings] (normal window)    │
└────────────────────────────────────────┼────────────────────────────────────────────┘
                                         │ framed stdin/stdout
           ┌─────────────────────────────┴───────────────────────────┐
  aural-stt-onnx.exe (Parakeet via ONNX Runtime)     aural-stt-ggml.exe (whisper.cpp; ggml
  CPU EP (v1); DirectML/CUDA EP later                 backends: CPU / Vulkan / CUDA loaded as DLLs)
```

**Data flow (one dictation):**
1. Hotkey down: the session goes to Listening. The mic stream opens, the pill appears, and level frames drive the bars.
2. Hotkey up: the session goes to Processing. The PCM (16 kHz mono f32) goes to the active worker, which returns the text.
3. The inserter checks the foreground target, runs its strategy, and the pill shows Success, then hides.
4. On any failure the text is copied to the clipboard, the pill shows a short error, and "Paste last transcript" appears in the tray menu (kept in memory only).

**Why separate STT worker processes (hybrid):**
- **Known Windows problem:** linking whisper-rs (whisper.cpp) and ONNX Runtime together causes MSVC CRT link conflicts. The Whispering app hit exactly this.
- **Crash isolation:** GPU driver faults and CUDA OOM kill the worker, not the app. The core restarts the worker and falls back.
- **Memory:** unloading a model means killing the process, which returns all RAM/VRAM to the OS.
- **Backends:** CUDA/Vulkan DLL loading is confined to the worker.

The cost is roughly 1–3 ms of IPC for a 10 s clip (640 KB), which is negligible.

## 2. Desktop framework — **Tauri v2 (Rust core + WebView2 UI)**

| Option | Verdict | Why |
|---|---|---|
| **Tauri v2** | **Chosen** | Rust core calls Win32 directly (hooks, SendInput, clipboard, DXGI). Native STT libraries are Rust/C. Transparent always-on-top windows work. Small installer (~10–20 MB before runtimes). NSIS/MSI bundler, tray, autostart, single-instance and updater plugins. Proven by Handy for this exact product. |
| Electron | Rejected | 150–250 MB installed. Hooks, audio and STT all need native addons anyway, so you get two runtimes (Node + native) for no gain. |
| .NET WPF | Rejected (close 2nd) | Good Win32 interop. But `AllowsTransparency` forces software rendering, NativeAOT for WPF is limited, and the STT libraries are C/C++/Rust, so we'd P/Invoke into the same native libs plus carry a CLR. |
| WinUI 3 | Rejected | Transparent borderless overlays are still awkward. Packaging (Windows App SDK) adds friction. |
| Qt | Rejected | LGPL/commercial licensing friction and C++. It offers nothing extra over Tauri for this app. |
| Pure-Rust UI (egui/Slint/iced) | Rejected for settings; **kept as pill fallback** | Tiny footprint. But a polished settings UI costs more effort. A native layered-window pill is the fallback if spike S1 fails. |

**Known Tauri risks:**
- The WebView2 pill could steal focus. Spike S1 tests this, and the fallback is a native layered window.
- WebView2 memory is roughly 50–150 MB. Mitigation: the settings window is destroyed when closed; only the pill webview stays alive.

**Frontend:** Svelte 5 + TS. Small runtime and little boilerplate. React was rejected as heavier with no benefit at this UI size. No component library; the design system is our own CSS tokens.

## 3. STT architecture — **pluggable engines; Parakeet TDT 0.6B v2 + whisper.cpp for v1**

The research matrix is summarized below. Sources: Open ASR leaderboard, model cards, Handy/OpenWhispr/Whispering source. Full sources are in §17.

| Engine | Eng. accuracy (avg WER) | CPU | NVIDIA | AMD (Windows) | Size | License (code/weights) | Streaming | Verdict |
|---|---|---|---|---|---|---|---|---|
| **Parakeet TDT 0.6B v2** (ONNX int8/fp32) | **~6.05%** (top tier, English-only) | **Excellent** (very high RTFx; practical on laptop CPU) | via ORT CUDA EP (heavy cuDNN) or CPU | CPU; DirectML EP experimental | int8 ≈0.65 GB, fp32 ≈2.4 GB (**verify**) | Apache-2.0 / **CC-BY-4.0** | Offline (fast enough for push-to-talk) | **v1 default engine** |
| **whisper.cpp** (ggml) | large-v3 ~6.4–7.4%; turbo a bit worse; small.en/base.en much worse | Good with AVX2 (small/base) | **CUDA** (fast) | **Vulkan: the only proven AMD GPU path on Windows** | base.en 0.15 GB · small.en 0.5 GB · turbo q5 ≈0.55 GB | MIT / MIT | Chunked pseudo-streaming | **v1 GPU engine + low-end CPU fallback** |
| faster-whisper / CTranslate2 | = Whisper | Good | CUDA | **No** (ROCm is Linux only) | = Whisper | MIT/MIT | No | Rejected: no AMD, Python-centric |
| Canary 1B-flash / v2 | Very good | Heavier than Parakeet | CUDA | CPU | 1–2 GB | CC-BY-4.0 (original canary-1b is **NC**; avoid) | No | Later candidate via the same ONNX worker |
| Moonshine | Vendor claims strong; **unverified** | Very fast (tiny) | — | CPU | tiny | MIT (legacy non-English models are NC) | **Native streaming** | Watch-list for live partials (post-v1) |
| Kyutai STT | Competitive | No | CUDA (candle) | No | 1–2.6 B | CC-BY-4.0 | True streaming | Rejected: no Windows story |
| Vosk | Much worse | Yes | — | CPU | small | Apache-2.0 | Yes | Rejected: accuracy |
| Windows built-in (Win+H / Voice Access) | Baseline | — | — | — | — | Proprietary | — | Not accessible as an engine; comparison baseline only |

**Decision:**
- **Parakeet TDT 0.6B v2** is the default. It is English-only (matches your scope), the most accurate practical English model, and fast on CPU.
- **whisper.cpp** is the second engine. It gives GPU acceleration on every vendor (CUDA and Vulkan) and tiny models for weak CPUs.
- **Whisper is not the default just because it is familiar.** It exists because it is the only mature AMD-GPU path and the best small-model fallback.

**Engine abstraction:** a core-side `SttEngine` trait plus a versioned IPC protocol. A new engine is a new worker adapter plus manifest entries, and the core does not change. v1 is offline (whole clip after release). The protocol reserves `AudioChunk`/`Partial` messages so streaming engines (Moonshine, Parakeet streaming) can be added later without breaking changes.

**Must benchmark before finalizing defaults (Milestone 0):**
- Parakeet v2 int8 vs fp32 accuracy (int8 missing-word reports exist).
- Parakeet CPU latency on the Intel laptop.
- whisper turbo on the 4070: CUDA vs Vulkan.
- The Parakeet-CPU vs Whisper-GPU trade-off on the 4070.

The architecture holds whatever the result; only the *default recommendations* change.

## 4. NVIDIA strategy
- **Always available:** Parakeet v2 on CPU. It may well be fast enough that GPU is unnecessary; the benchmark decides.
- **whisper.cpp Vulkan backend:** ships in the installer (small DLL), works on NVIDIA with no extra download.
- **whisper.cpp CUDA backend = optional "NVIDIA CUDA runtime pack"** (~0.6 GB: `ggml-cuda.dll` + cudart + cuBLAS/cuBLASLt, CUDA 12.x).
  - It is downloaded from Settings like a model and **only offered when the benchmark shows CUDA beating Vulkan by ≥30% latency on the 4070**. Otherwise we skip it and avoid the EULA/size burden.
- **CUDA EULA:** Attachment A allows redistributing cudart/cuBLAS inside an app with material added functionality, accessed only by our app. A separately downloaded pack fetched *by our app* is our reading of compliance. **This is not legal advice; flagged for review.** We ship the EULA text in the pack.
- **Detection:** DXGI vendor 0x10DE plus `nvcuda.dll` present plus a driver version check before offering CUDA.
- **Validation:** on your 4070 — model load time, VRAM (turbo q5 ≈1–1.5 GB, far under 8 GB), p50/p95 latency, WER, and a 1-hour soak (200 dictations, no leak or VRAM growth).
- **Post-v1:** Parakeet on GPU through the ORT CUDA EP (needs cuDNN, large) or TensorRT-RTX via Windows ML.

## 5. AMD strategy (honest version)
- **AMD is materially harder than NVIDIA or CPU on Windows.**
  - ROCm on Windows is preview-grade and only for RDNA3/4, so we won't build on it.
  - DirectML is in "sustained engineering" (maintenance only).
  - Windows ML's MIGraphX EP is Win 11 24H2+ only, and its Rust integration is immature.
- **v1 AMD path:**
  - Parakeet v2 on CPU (reliable).
  - whisper.cpp **Vulkan**. The Vulkan loader comes with the AMD driver, so there is no extra download.
  - Known risks: Windows AMD Vulkan drivers can be much slower than Linux, and Buzz has seen silent CPU fallback.
- **Self-test guard:** the first time a GPU backend is selected, the worker transcribes a bundled 5 s sample on both the GPU backend and CPU.
  - If the GPU fails to initialize or is slower, we warn and recommend CPU.
  - The result is shown to the user and can be copied into a "Hardware report" GitHub issue template.
- **Status label in the UI and docs:** "AMD GPU (Vulkan): community-validated / experimental". It stays marked **unverified** until real AMD reports arrive. CI only proves it *builds*.
- **Post-v1 experiments:** DirectML EP for Parakeet on AMD; WinML MIGraphX on 24H2+.

## 6. CPU strategy
- **Default:** Parakeet v2 (int8 or fp32, chosen by benchmark) on the ONNX Runtime CPU EP.
- **Weak CPUs:** under 4 physical cores, no AVX2, or under 8 GB RAM. For these, recommend whisper **base.en** or **small.en** (q8) via ggml. ggml's dynamic CPU backends pick the best build (AVX2/AVX-512) at runtime.
- **Threading:** use physical cores − 1 by default, capped at 8, to keep the foreground app responsive.
- **Model residency:** the model stays loaded in the worker while the app runs (~0.7–1 GB RAM for Parakeet int8, **measure**). A later setting "unload after N min idle" can trade that for first-use latency.
- **Intel Arc:** the ggml Vulkan backend happens to work on it. We list it as "Vulkan (other GPU): experimental" and do not promote it.

## 7. Model packaging / download strategy
- **Nothing large ships in the installer or git.** The installer contains the app, the two worker exes, ONNX Runtime CPU DLL, ggml CPU + Vulkan backend DLLs, and a 5 s self-test WAV (recorded by us, CC0).
- **The model catalog is a JSON manifest compiled into the app** (`manifests/catalog.v1.json`). The app does **not** fetch it at runtime, so no silent network use. It is updated with app releases.
- **Per-file catalog fields:** URL pinned to an **immutable revision** (Hugging Face `resolve/<commit-sha>/…`), size, sha256, license id and attribution text.
- **Per-model catalog fields:** engine, compatible backends, min RAM/VRAM, languages.
- **Model sources:**
  - **Parakeet:** the upstream ONNX conversions on HF (sherpa-onnx/onnx-asr exports; exact repo chosen during M0 by license, format and hash).
  - **Whisper:** `ggerganov/whisper.cpp` ggml files on HF.
- **Runtime packs** (CUDA) are GitHub Release assets of this project, with sha256 in the manifest.
- **Storage layout:**
  - Models: `%LOCALAPPDATA%\Aural\models\<model-id>\`.
  - Runtimes: `%LOCALAPPDATA%\Aural\runtimes\<pack-id>\`.
  - Each has an `installed.json` receipt with version, sha256 and install time.
- **Download mechanics:**
  - Downloads go to `*.partial` with HTTP Range resume.
  - Each file is sha256-verified, then atomically renamed.
  - The receipt is written last, so a model without a receipt counts as not installed.

## 8. Audio pipeline
- **Capture:** `cpal` (WASAPI shared) opens the selected device, or the default device resolved at *each* hotkey press, which handles device changes.
- **Processing:** downmix to mono, then `rubato` resamples to 16 kHz f32, all in a ring-buffered capture thread. The mic stream **opens on hotkey down and closes on finish** (privacy: the Windows mic indicator is only on while dictating).
- **Mic-open latency:** likely 50–150 ms, so the first syllable could be clipped. Measured in M1. Mitigations:
  - (a) Start capture on key-down before any UI work.
  - (b) An optional "Keep microphone ready" setting, **off by default**, that keeps the stream open with a 300 ms pre-roll.
- **Level analysis for the pill:** every 33 ms, take a 1024-sample FFT (`realfft`) and compute 12 log-spaced bands from 90 Hz to 4 kHz. Apply dB normalization, then attack 25 ms / release 160 ms smoothing, and send it as a `LevelFrame { bands: [f32; 12], rms }` event. This is **real audio, not a loop**.
- **Silence and "tap" guards:**
  - Recordings under 250 ms are discarded.
  - If whole-clip RMS stays under the noise gate, the result is `TranscriptEmpty`: nothing is inserted and the pill hides quietly. This blocks Whisper's "Thank you." hallucinations.
  - Leading and trailing silence is trimmed with the energy gate. Silero VAD (MIT) comes post-v1 if needed.
- **Limits:** max recording is 5 min (configurable). At the limit we auto-stop and transcribe. Memory is about 19 MB at 16 kHz f32.
- **Mic privacy denial:**
  - Before capture, read the `CapabilityAccessManager\ConsentStore\microphone` values (HKCU/HKLM, including `NonPackaged`).
  - Also detect all-zero capture.
  - On either, show the error "Microphone blocked" with a button that opens `ms-settings:privacy-microphone`.

## 9. Global hotkey implementation
- **Mechanism:** a **WH_KEYBOARD_LL** hook on a dedicated thread with its own message loop, using the `windows` crate. It sees key-up events (push-to-talk), modifier-only chords (e.g. Ctrl+Win) and single keys (Right Ctrl), and can optionally swallow the chord.
  - The callback does nothing except `try_send` to a channel, because Windows silently removes slow hooks (~1 s LowLevelHooksTimeout).
  - Injected events (`LLKHF_INJECTED`) are ignored, so our own SendInput never re-triggers the hotkey.
- **Hook health:** every 5 s the thread checks a heartbeat. If the hook was silently removed, it re-installs it, and after repeated failure the tray shows an error.
- **Rejected alternatives:**
  - `RegisterHotKey` / `tauri-plugin-global-shortcut`: no real key-up (50 ms polling emulation), no modifier-only chords, and a known deadlock.
  - Raw Input: cannot swallow keys.
  - The `handy-keys` crate: we evaluate it in M1 and adopt it if it fits. Otherwise ~300 lines of our own code.
- **Default hotkey:** hold **Ctrl+Win**.
  - On release we send a **mask key** (unassigned VK 0xE8) so Windows doesn't open Start.
  - The chord is swallowed only while it matches exactly, so Ctrl+Win+→ (desktop switching) still works: any extra key cancels the match and passes through.
- **Conflicts:**
  - The recorder UI refuses reserved combos: Win+L, Ctrl+Alt+Del, Win+G, **Win+H** (Windows voice typing), Ctrl+Win+Enter/O, and the Copilot key's Win+Shift+F23.
  - It warns on common app shortcuts (Ctrl+C/V/Z/S, Alt+Tab, Alt+F4).
  - There is no OS-level way to detect every other app's hotkeys. We document that and make it easy to change.
- **Modes:** **push-to-talk** (default) and **toggle** (press to start, press to stop) in v1. Both are tiny variations inside the pure session state machine. "Hands-free with VAD auto-stop" and "double-tap to lock" are post-v1.
- **Esc cancels** a listening session (discard audio). This is done in-hook, only while Listening.
- **Limitation (documented):** when an **elevated** (admin) window has focus, the hook doesn't receive keys from it (UIPI). Dictation won't start over admin apps unless Aural itself runs elevated, which we do not recommend.

## 10. Windows text insertion strategy
Strategies form a chain chosen per target by a pure `plan_insertion(target, settings)` function:

1. **ClipboardPaste (default).**
   - Snapshot the clipboard: CF_UNICODETEXT plus HTML/RTF if present, and a flag if other formats exist.
   - Set our text with `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory=0` and `CanUploadToCloudClipboard=0`, so dictation doesn't pollute Win+V history or cloud sync.
   - Send the paste chord, wait until `GetClipboardSequenceNumber` is stable plus a 200 ms grace, then restore the snapshot. We only restore if nobody else wrote to the clipboard meanwhile.
   - **If the snapshot holds non-restorable formats** (images, files, Office delayed render), switch to **UnicodeTyping** instead, so we never destroy the user's clipboard.
2. **UnicodeTyping.** A single `SendInput` batch of `KEYEVENTF_UNICODE` events, with surrogate pairs for emoji. Newlines become Shift+Enter in chat-class apps or are stripped (setting). Used as fallback, or by user choice or per-app rule.
3. **ClipboardOnly.** Leave the text on the clipboard and show "Copied — press Ctrl+V". Used when injection is impossible.

**Pre-insert guards (all targets):**
- **Modifier guard:** wait up to 300 ms for all physical modifiers to be released (`GetAsyncKeyState`). If they are still down, send synthetic key-ups plus the mask key. Otherwise Ctrl+V becomes Ctrl+Win+V.
- **Focus guard:** record the foreground HWND at hotkey release. If the foreground changed before insertion, use ClipboardOnly and show "Copied" rather than typing into the wrong window.
- **Elevation guard:** if the target process is elevated and we are not (token check), UIPI will silently drop input, so use ClipboardOnly with the message "Admin window: copied instead".
- **Secure desktop / lock screen:** there is no foreground HWND to inject into, so use ClipboardOnly.

**Target classes** (by process name and window class, as data in a table):

| Class | Examples | Default behaviour |
|---|---|---|
| Terminal | WindowsTerminal, conhost, mintty, Alacritty, WezTerm, VS Code integrated terminal (**not detectable by window**; documented) | Paste with **Ctrl+Shift+V** for WT, **Ctrl+V** for conhost. **Strip all trailing newlines; collapse internal newlines to spaces**, because multi-line paste can execute commands. |
| Chromium/Electron | Chrome, Edge, Brave, VS Code, Slack, Discord, Teams, Obsidian | Ctrl+V paste. Never type `\n`, because Enter sends messages. |
| Office/rich text | Word, Outlook, OneNote | Ctrl+V with plain text only, so it takes the destination formatting. |
| Native classic | Notepad, dialogs | Ctrl+V. |
| Remote/VM | mstsc, VMware, VirtualBox, Parsec | Ctrl+V; works if clipboard redirection is on, otherwise we tell the user. |
| Games/anti-cheat | fullscreen exclusive and known titles | **Unsupported.** ClipboardOnly. |

- **Password fields:** unfocused UIA `IsPassword` is checked best-effort; if true, we insert via typing only and never leave text on the clipboard.
- **Rejected methods:**
  - UIA `ValuePattern.SetValue` replaces the whole field and breaks undo.
  - `WM_CHAR`/`WM_PASTE` posting doesn't reach Chromium/Electron renderers.
  - `EM_REPLACESEL` only works for legacy Edit controls. It might become a later strategy.
- **Per-app override table:** process name → strategy/chord/newline policy. Built-ins ship in v1 and live in `settings.json`; a UI for editing it comes in M5.
- **`uiAccess` manifest** (injecting into admin windows): requires a signed exe installed in Program Files. **Deferred** until we have code signing.

## 11. Floating UI architecture
- **Window:** a dedicated Tauri window `pill`, created hidden at startup (avoids first-show lag).
  - Properties: `transparent`, `decorations:false`, `always_on_top`, `skip_taskbar`, `focusable:false`, `shadow:false`.
  - After creation we add `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT` (click-through) to the raw HWND and show it with `SW_SHOWNOACTIVATE`.
  - Click-through is disabled only in the Error state, so the error pill is clickable to open help.
- **Placement:** bottom-center of the monitor that holds the foreground window, 48 px above the work-area bottom (taskbar-aware, per-monitor DPI). Top-center is an option.
- **Rendering:** one `<canvas>` at devicePixelRatio, drawn with `requestAnimationFrame` from the latest `LevelFrame` values. The CSS handles only the pill shape and state transitions. Animations use only transform/opacity, and there is a `prefers-reduced-motion` path.
- **State:** the Rust session machine emits `PillState` = `Hidden | Listening | Processing | Success | Error{code}`. The pill UI is a pure view of that state; a small TS reducer is tested with Vitest.
- **Fallback (if spike S1 shows focus stealing, flicker or >16 ms frame jitter):** a native layered window (`UpdateLayeredWindow` + `tiny-skia`) that renders the same design in Rust. The design is simple enough to port.

### 11a. Visual design — `/reimagine-it` pass (plan-only)

`REIMAGINED: partial`. Plan mode forbids running `npx reimagine-it`, which writes files and fetches a package. The directions below come from the skill's plan-only flow. **Rendered variants are Task M2.1:** write `design/pill-states.html` and `design/settings.html` mocks, then run `npx reimagine-it variations -i design/pill-states.html -n 3 -o design/reimagined/pill/` (same for settings), then `npx reimagine-it audit` on the chosen file.

Anchors from the product: *hotkey held · voice level · "processing" · text landed · error*.

**Pill directions explored:**

| Dir | Idea | Critique |
|---|---|---|
| **A. Monolith** | Solid near-black capsule, white mark left, 12 mirrored hairline bars from a centre line; states change **bar behaviour**, not colour. | Reads on any background (with hairline border). Calm and distinctly "instrument". Risk: plain → solved with precise motion. **Selected.** |
| B. Oscilloscope | One 1.5 px continuous waveform line. | Elegant but reads "audio editor". Low-level speech looks flat, and you asked for bars. |
| C. Dot-matrix | Bars made of 4×12 LED dots (hardware / Teenage-Engineering feel). | Charming, but noisy at 36 px height and harder to read at low levels. We keep its *idle dots* idea inside A. |
| D. Acrylic glass | Translucent Mica/acrylic pill. | Blur behind a transparent WebView is unreliable, Win10 doesn't support it, and it drifts toward the "AI glow" aesthetic. Rejected. |

**Selected: A "Monolith" (with C's idle dots).**
- **Geometry:** 176×36 px at 100% scale, fully rounded.
- **Colours:** fill `#0B0B0B`, 1 px inner hairline `rgba(255,255,255,.10)` (separates it from dark apps), outer shadow `0 6px 24px rgba(0,0,0,.28)` (separates it from light apps).
- **Logo:** original 18 px white monogram (a vertical stroke + open arc, "voice into text cursor"), 14 px from the left edge. Designed in M2; nothing borrowed from Wispr Flow.
- **Bars:** 12 bars, 2 px wide, 3 px gap, rounded caps, mirrored vertically around the centre line, heights from band energies.
- **Listening:** bars move with real audio; the minimum height is a 2 px dot, so silence shows a calm row of dots.
- **Processing:** bars settle to dots, and a soft white highlight **sweeps across the dots** left to right (900 ms period, opacity only). Reads as "reading what you said", not a spinner.
- **Success:** dots **join into one hairline** (scaleX), hold 250 ms, then the pill fades and drops 4 px (160 ms). No green, no checkmark emoji.
- **Error:** bars are replaced by an 11 px label in `#A3A3A3` ("Mic blocked", "No model", "Copied — Ctrl+V"). The pill nudges ±2 px once and stays 2.5 s. It is clickable and opens the relevant Settings page.
- **Enter:** opacity 0→1 and scale .94→1 over 120 ms ease-out. **Exit:** 160 ms. **Reduced motion:** instant state swaps, and the bars show levels without smoothing flourish.
- **Size:** optional "Compact" size at 128×28 px (UI preference).

**Settings directions explored:**
1. **Win11 Settings clone** (cards, Fluent icons). Native, but generic and heavy.
2. **Instrument manual** (numbered sections, monospace numerals, hairline rules). Distinctive, but slightly precious for toggles.
3. **Utility sheet.** Compact window, left text-nav, grouped rows with hairline dividers, no cards, no shadows, inversion (black↔white) as the only "accent".

**Selected: 3 with 2's typographic discipline.**
- **Window:** 760×540, Mica backdrop on Win11 (solid on Win10), follows the system light/dark theme, grayscale tokens only.
- **Type:** system `Segoe UI Variable` (no webfont, offline, native), tabular numerals for sizes, speeds and latencies.
- **Nav:** General · Audio · Models · Insertion · Appearance · Privacy & About.
- **Models page:** a real table — name, engine, size, RAM/VRAM, backend badges, state (Installed / Active / Downloading 43% / Available), with Download/Remove/Use actions and a "Recommended for this PC" row marker.
- **Stretch (the reimagine "adjacent possible"):** the **Audio page's mic test uses the actual pill component inline**. You see the exact live bars you'll get while dictating, and the same component doubles as hotkey-recorder feedback. It is cheap because the component is reused.
- **Kill list:** gradients, purple "AI" glow, sparkles, chat bubbles, dashboard stat cards, emoji status.

## 12. Settings architecture
- Rust is the single source of truth. `Settings` is a serde struct with `#[serde(default)]` on every field plus a `schema_version: u32`.
  - Stored as `%APPDATA%\Aural\settings.json`.
  - Written atomically (temp file + rename).
  - Migrated by an ordered list of `fn(Value)->Value` steps.
  - A corrupt file is renamed `settings.corrupt-<ts>.json` and defaults are loaded, with a notice.
- **Sections (v1):**
  - `hotkey` {chord, mode, swallow}
  - `audio` {device_id|default, keep_ready, max_seconds}
  - `stt` {active_model_id, backend_pref}
  - `insertion` {default_strategy, restore_clipboard, newline_policy, app_rules[]}
  - `ui` {pill_position, pill_size, theme}
  - `startup` {launch_at_login, start_hidden}
  - `privacy` {log_transcripts:false}
- **UI ↔ core:** typed Tauri commands (`get_settings`, `update_settings(patch)`) and a `settings-changed` event. TS types are generated from Rust with `tauri-specta` (**verify v2 compatibility at M3**; fallback is hand-written `ts-rs` types).
- **Extensibility:** new sections are new fields with defaults. The UI renders pages per section, and there is no generic plugin system (YAGNI).

## 13. Model manager architecture
- **Modules (`aural-models` crate):**
  - `catalog` — parse and validate the manifest.
  - `store` — scan install dirs and read receipts.
  - `download` — resumable, verifying, cancellable, progress events.
  - `compat` — `(ModelEntry, HardwareProfile) -> Compatibility {Recommended|Compatible|Unsupported(reason)}`.
  - `service` — the facade used by Tauri commands.
- **Per-model state the app knows:** installed?, compatible?, active?, size, approx RAM/VRAM, download progress/state, **removable?** (not active, and the worker isn't using it), license and attribution.
- **Removal:** refuses while active or in use. It deletes the receipt first, then the files, so a crash mid-delete leaves "not installed" rather than a half-model.
- **Switching models:** stop the worker, start it with the new model, then run the self-test.
- **Network visibility:** every network call goes through one `NetClient` that logs host + purpose to an in-app "Network activity" list (Privacy page) and shows a download indicator in the tray.

## 14. Hardware detection
- **GPUs:** DXGI `EnumAdapters1` gives vendor id (0x10DE NVIDIA, 0x1002 AMD, 0x8086 Intel), name, `DedicatedVideoMemory`, and a software-adapter flag.
- **Runtime availability:** NVIDIA driver/CUDA via `nvcuda.dll` + `cuDriverGetVersion` loaded dynamically; Vulkan via `vulkan-1.dll` present. The worker's device enumeration is authoritative at self-test.
- **CPU and RAM:** `is_x86_feature_detected!` (avx2, avx512f, fma), physical cores, `GlobalMemoryStatusEx` total/available RAM. Also OS build, used to gate Win11-only features.
- **Recommendation:** a pure `recommend(profile, catalog) -> Vec<(ModelId, Backend, Rank)>`, table-driven and unit-tested.
  - Always includes CPU options.
  - It never *blocks* choosing another compatible option; unsupported ones show the reason.

## 15. Error handling
One `DictationError` enum in core; each variant maps to (pill label ≤ 20 chars, Settings deep-link, log level, recovery):

| Error | Recovery |
|---|---|
| MicBlocked / MicUnavailable / MicDisconnected | Deep-link to privacy settings / device picker; re-resolve device next press |
| NoModelInstalled / ModelMissingFiles | Pill "No model", click opens Models page |
| WorkerCrashed / WorkerTimeout (30 s + 2 s per audio second) | Auto-restart once, retry same audio once, then error and keep audio in memory for "Retry last" |
| BackendInitFailed (CUDA/Vulkan) | Fall back to CPU for this session; notice suggesting CPU backend |
| TranscriptEmpty | Silent hide (not an error to the user) |
| InsertBlockedElevated / FocusChanged / InsertFailed | ClipboardOnly + "Copied — Ctrl+V"; tray "Paste last transcript" |
| HookRemoved / HotkeyConflict | Re-install; tray warning; recorder validation |
| DownloadFailed / ChecksumMismatch / DiskFull / Cancelled | Keep `.partial` for resume (except checksum → delete); clear message with sizes |
| SettingsCorrupt | Backup + defaults + notice |

**Invariant:** a successful transcript is never silently lost. Logs are written with `tracing` to rolling files at `%LOCALAPPDATA%\Aural\logs`. **Transcript text and audio are never logged** unless `privacy.log_transcripts` is on (off, with a warning label).

## 16. Privacy architecture
- No account, no telemetry, no crash upload, no analytics. **Adding any requires explicit discussion** (it goes in CONTRIBUTING as a rule).
- **Audio:** kept in memory only, never written to disk, freed after transcription. The mic is open only while dictating by default.
- **Network:** only for user-initiated model/runtime downloads and an optional **manual** "Check for updates" (no background update checks in v1). All of it goes through `NetClient`, is visible in the Network activity list, and is enforced by a **Tauri CSP** that blocks webview network access entirely (`connect-src ipc: http://ipc.localhost`).
- **Clipboard:** history and cloud exclusion formats are set, and the clipboard is restored.
- Last transcript is kept in memory only for "Paste last transcript", and cleared on exit.
- `PRIVACY.md` documents all of the above, including exactly which hosts can be contacted (huggingface.co, github.com).

## 17. Dependency / license analysis
**Project license: MIT OR Apache-2.0 (dual).**
- It is the Rust ecosystem norm, matches Tauri, windows-rs and ort, and qualifies for SignPath Foundation free OSS code signing.
- **No GPL dependencies are permitted.** `cargo-deny` enforces an allow-list: MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, Unlicense, MPL-2.0 (file-level, review each case).

| Component | License | Obligation |
|---|---|---|
| Tauri v2, windows-rs, arboard, ort | MIT/Apache-2.0 | Notices |
| whisper.cpp/ggml | MIT | Notice |
| whisper-rs | Unlicense (dev moved to Codeberg; GitHub archived) | Monitor maintenance; fallback: bind whisper.cpp C API directly |
| transcribe-rs | MIT | Notice; single-maintainer risk → wrapped behind our adapter |
| ONNX Runtime | MIT (+ third-party notices) | Ship ORT ThirdPartyNotices |
| cpal (Apache-2.0), rubato (MIT), realfft (MIT) | permissive | Notices (**confirm via cargo-about**) |
| Whisper weights (ggml) | MIT | Attribution in MODELS.md |
| **Parakeet TDT 0.6B v2 weights** | **CC-BY-4.0** | Credit NVIDIA, model name, license link, source link, and **"converted to ONNX/int8" modification notice** in About + MODEL_LICENSES.md |
| CUDA runtime (cudart, cuBLAS/Lt) | NVIDIA CUDA EULA (Attachment A redistributable) | Only inside our optional pack, used only by our app, EULA included; **legal uncertainty flagged** |
| Silero VAD (post-v1) | MIT | Notice |
| WebView2 runtime | Microsoft | Installed by bootstrapper (allowed) |
| **Avoid:** canary-1b (CC-BY-NC), legacy non-English Moonshine (non-commercial), Llama models (custom license), anything GPL/AGPL/NC | — | — |

**Tooling:**
- `cargo-about` generates `THIRD_PARTY_NOTICES.html` for Rust deps, and `license-checker` (or similar) covers npm deps.
- `MODEL_LICENSES.md` is hand-maintained, and the catalog carries per-model license and attribution fields that the About page renders.

**MiniMind (jingyaogong/minimind) — investigated:**
- It is an Apache-2.0 *educational* project for training tiny LLMs from scratch (64M dense / 198M MoE, mainly Chinese data).
- The main repo is text-only. minimind-v adds vision. minimind-o is a 0.1B voice *chatbot* that uses a frozen SenseVoice encoder, not a transcriber.
- **STT:** no. There is no ASR capability, and minimind-o's speech metrics measure its spoken replies, not transcription.
- **Correction / formatting / post-LM:** no. At 64M parameters with weak English it would hallucinate and change meaning. That violates dictation's core promise (text = what you said).
- **Where it could fit:** only as a *learning resource* if you later want to train a tiny domain-specific punctuation or command classifier.
- **For v1 you don't need an LM at all:** Parakeet and Whisper already output punctuation and casing.
- **If post-v1 "cleanup mode"** (filler removal, formatting) is wanted, the right candidates are Qwen3 0.6B/1.7B (Apache-2.0) via llama.cpp/ggml. It would run in the existing ggml worker, **opt-in and off by default**. Gemma has custom terms and Llama needs attribution plus an AUP, so both are less attractive.

## 18. Testing strategy
**TDD (unit tests, run everywhere; the pure logic is deliberately isolated from Win32):**
- Session state machine: every transition, both modes, cancel, tap-discard, max-duration.
- Hotkey chord matcher: synthetic key streams, extra-key cancel, mask-key emission decision, injected-flag ignore.
- Audio: downmix, resample length/rate, level bands against synthetic sines, silence gate.
- STT: IPC codec round-trip and framing errors; `SttClient` with a **fake worker exe** (crash, timeout, restart).
- Models: catalog validation, compat/recommend tables, download resume/sha256/atomic-rename against a local `hyper` test server, removal guards.
- Insertion: `plan_insertion` tables (class × settings × guards), newline policies, surrogate-pair encoding.
- Settings: defaults, migrations, corrupt-file recovery, atomic write.
- Hardware: `recommend()` against fixture profiles (4070, AMD 7800 XT, iGPU laptop, 4-core no-AVX2).
- UI: Vitest for the pill reducer and bar-mapping math.

**Integration (Windows, real OS; some in CI):**
- Worker integration tests with a real tiny model (whisper tiny.en, ~75 MB, cached in CI). Parakeet tests are gated behind an env var locally.
- **Insertion harness:** a tiny test-target app (Win32 Edit control + WebView2 textarea) that records what it receives. Automated SendInput/clipboard tests run against it. It is unclear whether GitHub's windows runners have an interactive desktop that accepts SendInput, so this runs **locally first** and moves to CI only if it proves reliable.
- Hook install/uninstall smoke test.

**Manual (cannot be automated honestly):** a checked-in `docs/testing/insertion-matrix.md` filled in per release:
- Notepad, Word, Outlook, Chrome, Edge, Firefox (textarea + contenteditable + Google Docs)
- VS Code editor and integrated terminal, Slack, Discord, Teams, Obsidian
- Windows Terminal (pwsh, cmd, WSL), legacy conhost, JetBrains IDE
- Elevated Notepad, a password field, an RDP session, a game

Also manual: 4070 CUDA/Vulkan benchmarks + 1 h soak; Intel laptop CPU benchmark; AMD via community reports.

**Benchmarks:** `tools/bench` measures WER, latency p50/p95, RTF, peak RAM/VRAM, and load time on the fixed corpus. Results are committed as markdown under `docs/benchmarks/`.

## 19. Build and installer strategy
- **Installer:** Tauri NSIS, **per-user install** (no admin, `%LOCALAPPDATA%\Programs\Aural`).
  - WebView2 via `embedBootstrapper` (Win10 may lack WebView2).
  - Autostart via `tauri-plugin-autostart` (HKCU Run key), plus `tauri-plugin-single-instance`.
  - **Target installer size <25 MB.**
- **Workers:** built as separate binaries and bundled as Tauri `externalBin` sidecars.
  - ggml is built with `GGML_BACKEND_DL=ON` + `GGML_CPU_ALL_VARIANTS=ON` + Vulkan → CPU-variant DLLs + `ggml-vulkan.dll` in the installer.
  - `ggml-cuda.dll` is built separately into the CUDA runtime pack.
  - **Verify at M4** that whisper-rs exposes these CMake options; fallback is building whisper.cpp via CMake in `build.rs` ourselves.
- **Build prerequisites (documented):** VS 2022 Build Tools (MSVC + Windows SDK), Rust stable (pinned in `rust-toolchain.toml`), Node LTS + pnpm, CMake, **LLVM** (libclang for whisper-rs bindgen; its bundled bindings are Linux-only; set `LIBCLANG_PATH`), **Vulkan SDK** (for the ggml Vulkan build), **CUDA Toolkit 12.x** (only for building the CUDA pack).
- **Code signing:** unsigned for early releases (SmartScreen "unknown publisher", documented). Apply to the **SignPath Foundation** (free for OSI projects) before v1.0. Azure Artifact Signing (~$10/mo, US/Canada individuals) is the alternative.
- **Updates:** v1 uses a manual "Check for updates" that opens the GitHub Releases page. `tauri-plugin-updater` with signed manifests comes post-v1 once signing exists.

## 20. GitHub / open-source structure
```
aural/
├─ Cargo.toml                    # workspace
├─ rust-toolchain.toml  deny.toml  about.toml  .gitignore  .gitattributes  .editorconfig
├─ LICENSE-MIT  LICENSE-APACHE  README.md  CONTRIBUTING.md  SECURITY.md  PRIVACY.md
├─ CODE_OF_CONDUCT.md  MODEL_LICENSES.md  CHANGELOG.md
├─ crates/
│  ├─ aural-core/        # session state machine, errors, settings types (pure, no Win32)
│  ├─ aural-platform/    # Win32: hotkey hook, insertion, hw detect, mic privacy, foreground/elevation
│  ├─ aural-audio/       # cpal capture, resample, level analyzer, gates
│  ├─ aural-models/      # catalog, store, download, compat/recommend
│  └─ aural-stt-protocol/# IPC messages + codec + SttClient
├─ workers/
│  ├─ stt-onnx/           # aural-stt-onnx.exe (Parakeet)
│  └─ stt-ggml/           # aural-stt-ggml.exe (whisper.cpp)
├─ app/                   # Tauri app
│  ├─ src-tauri/          # main.rs, orchestrator, commands, tray, windows
│  └─ ui/                 # Svelte: pill/, settings/, design tokens
├─ manifests/catalog.v1.json
├─ tools/bench/           # aural-bench (dev tool, not shipped)
├─ tools/insert-target/   # test target app for insertion harness
├─ design/                # mocks, reimagine-it outputs, logo SVG
├─ docs/  architecture.md  development.md  building-windows.md  models.md  releasing.md
│        benchmarks/  testing/insertion-matrix.md  superpowers/specs|plans/
└─ .github/ workflows/ci.yml release.yml  ISSUE_TEMPLATE/{bug,hardware-report,insertion-failure}.yml
           PULL_REQUEST_TEMPLATE.md  dependabot.yml
```
**`.gitignore` must exclude:** `target/`, `node_modules/`, `dist/`, `*.onnx`, `*.bin`, `*.gguf`, `*.ggml`, `models/`, `runtimes/`, `*.partial`, `*.wav` (except `app/src-tauri/resources/selftest.wav`, which is allow-listed), `.env*`, `*.pfx`, `*.pem`, and IDE folders. A `pre-commit`-style CI check fails on any file >5 MB.

## 21. CI strategy (GitHub Actions)
- **`ci.yml`** runs on PR/push on `windows-latest`:
  - `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace` (pure + Win32 unit tests).
  - `cargo deny check` (licenses, advisories, bans).
  - `pnpm lint && pnpm test` (Vitest) and `pnpm build`.
  - Large-file guard.
  - Worker build with **CPU+Vulkan** (Vulkan SDK via an install action; it only proves AMD/Vulkan *compiles*).
  - Cached whisper tiny.en integration test.
- **`release.yml`** runs on tag `v*`:
  - Builds the NSIS installer, THIRD_PARTY_NOTICES and SHA256SUMS, and uploads a **draft** release.
  - A separate job builds the CUDA runtime pack (CUDA toolkit action).
  - Nothing is published without manual review. Signing is added when available.
- **No GPU runners:** CUDA/Vulkan runtime correctness is validated manually on your 4070, and AMD by the community.

## 22. Development milestones
| M | Deliverable | Exit criteria |
|---|---|---|
| **M0 Foundations + evidence** | Repo bootstrap; `aural-bench`; benchmark report; spikes S1 (pill focus) and S2 (insertion) | Benchmark numbers on the Intel laptop + 4070; default engine/backends decided; S1/S2 go/no-go written up |
| **M1 Core loop (first usable)** | Tray app: Ctrl+Win push-to-talk → mic → Parakeet (ONNX worker) → ClipboardPaste with guards → focused app | Dictate into Notepad, Chrome, VS Code, Slack, Windows Terminal; release→text p50 < 1 s on the Intel laptop for a 10 s clip (**target, validate**) |
| **M2 Pill** | reimagine-it variants rendered; logo; pill window + real level bars + 5 states | No focus steal (S1 checklist); 60 fps on the laptop; reduced-motion path |
| **M3 Models + hardware + settings shell** | Catalog, download/resume/verify/remove, recommend, Models page, settings persistence | Fresh install → recommended model downloaded and working without touching files |
| **M4 whisper.cpp worker + GPU** | ggml worker (CPU/Vulkan), CUDA pack, self-test, fallback | 4070 CUDA + Vulkan benchmarked; 1 h soak clean; Vulkan builds in CI |
| **M5 Settings complete + insertion hardening** | Hotkey recorder + conflicts, toggle mode, mic picker + inline pill test, insertion options, app rules, UnicodeTyping, elevation/focus guards, startup | Insertion matrix filled; all Review Focus cases pass |
| **M6 Release 0.1** | Docs set, notices, installer, CI release, issue templates | Clean-VM install on Win10 22H2 + Win11; draft GitHub release reviewed by you |

Each milestone gets its **own detailed writing-plans pass** when it starts (Part B details M0 and M1 only), runs in its own worktree/branch, and ends with requesting-code-review before merge.

## 23. Risks and failure modes
| Risk | Likelihood / impact | Mitigation |
|---|---|---|
| Insertion fails in specific apps | High / High | Strategy chain, per-app rules, ClipboardOnly never-lose invariant, insertion-failure issue template |
| WebView pill steals focus / flickers | Med / High | Spike S1 before M2; native layered-window fallback |
| Low-level hook silently removed / AV flags hooks + SendInput | Med / High | Tiny callback, heartbeat reinstall; code signing; document AV false positives |
| AMD Vulkan slow or broken on Windows drivers | High / Med | Self-test + CPU fallback + honest "experimental" label |
| Parakeet int8 drops words | Med / Med | M0 benchmark int8 vs fp32; ship whichever passes |
| whisper-rs maintenance (moved to Codeberg) | Med / Med | Adapter boundary; fallback to direct whisper.cpp C API binding |
| transcribe-rs single maintainer / API churn | Med / Med | Pin version; adapter boundary; fallback: direct `ort` + our own TDT decoder (~400 LOC, documented) |
| CUDA EULA interpretation | Low / Med | Pack only if benchmark justifies it; legal-review flag; Vulkan works without it |
| Mic-open latency clips first word | Med / Med | Measure in M1; start capture first; optional keep-ready |
| Ctrl+Win default conflicts with user tools | Med / Low | Easy rebinding; Right Ctrl suggested alternative |
| Unsigned binaries → SmartScreen/AV friction | High / Med | SignPath application pre-1.0; documented |
| WebView2 missing on Win10 | Low / Med | Embedded bootstrapper |
| Model host (HF) URL changes | Low / Med | Pinned commit URLs + sha256; catalog updated per release |
| Scope creep (LLM cleanup, streaming, commands) | Med / Med | Section 24 hard list |

## 24. NOT in v1
- LLM post-processing, cleanup, rewriting, or "command mode" (MiniMind or any other LM).
- Live streaming partial transcripts in the pill.
- Multilingual models, language switching, translation.
- Hands-free / VAD auto-stop / wake word; custom vocabulary / boosting; snippets.
- Transcript history, search, or storage.
- Auto-updater, telemetry, crash reporting, accounts, sync, cloud STT fallback.
- Parakeet-on-GPU (CUDA/DirectML/WinML), ROCm, Intel OpenVINO/NPU backends.
- `uiAccess` elevated injection; ARM64 Windows build; macOS/Linux.
- Per-app override *editor UI* before M5; theming beyond light/dark system follow.

## 25. Specific first implementation task
**M0 · Task 0.1 → 0.5 below: bootstrap the repo, then build `aural-bench` and produce the STT baseline benchmark on your two machines.**

This comes first because the only architecture-affecting unknowns are numeric: Parakeet int8 vs fp32, CPU latency on the laptop, and CUDA vs Vulkan on the 4070. Everything else is designed. Spikes S1/S2 run in parallel worktrees.

## Open decisions for your review
1. **Name** (placeholder "Aural"; check GitHub/trademark collisions).
2. **Fork Handy instead?** (faster; less original). This plan assumes **no**.
3. **Default hotkey:** hold Ctrl+Win (Wispr-like) vs hold Right Ctrl.
4. **Project folder** for the real repo, e.g. `%USERPROFILE%\code\aural`. The current session folder is temporary.

---

