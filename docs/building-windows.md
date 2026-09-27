# Building Aural on Windows

## Prerequisites

| Tool | Why | Install |
|---|---|---|
| Visual Studio 2022+ Build Tools (C++ workload, Windows SDK) | MSVC compiler, and the C++ runtime DLLs shipped with the app | Visual Studio Installer |
| Rust (version pinned in `rust-toolchain.toml`) | everything | `winget install Rustlang.Rustup` |
| Node.js 24 LTS | settings window and pill (Svelte + Vite) | `winget install OpenJS.NodeJS.LTS` |
| CMake | builds whisper.cpp | `winget install Kitware.CMake` |
| LLVM | libclang for whisper-rs bindgen (its bundled bindings are Linux-only) | `winget install LLVM.LLVM`, then set `LIBCLANG_PATH=C:\Program Files\LLVM\bin` |

ONNX Runtime is downloaded by the `ort` crate during the build. `.cargo/config.toml`
sets `CL=/O2`; without it whisper.cpp is compiled unoptimized on MSVC. If `CL` is
already set in your shell, that value wins, so unset it.

## Layout

- `crates/aural-core` — settings, paths, dictation state machine, Delete Aural plan (no Win32)
- `crates/aural-audio` — microphone capture, resampling, level bands, silence gate
- `crates/aural-engines` — Parakeet (`onnx` feature) and Whisper (`ggml` feature) adapters
- `crates/aural-models` — model catalog (`manifests/catalog.v2.json`), downloads, installs
- `crates/aural-platform` — Windows: hotkey hook, text insertion, autostart, mic consent
- `crates/aural-stt-protocol` — worker protocol, client, fake worker for tests
- `workers/stt-onnx`, `workers/stt-ggml` — speech worker processes (one per engine family)
- `app/` — Tauri app: `src-tauri/` (Rust) and `src/` (Svelte)
- `tools/bench` — benchmark tool (not shipped)

## Everyday commands

```powershell
cargo test                      # libraries, bench, protocol (no app, no engines)
cargo clippy --all-targets -- -D warnings
```

The two workers enable mutually exclusive engine features, so build them one at a time
and never use `--workspace`:

```powershell
cargo build -p aural-stt-onnx
cargo build -p aural-stt-ggml
```

The app needs the frontend build and the staged sidecars first:

```powershell
./scripts/prepare-bundle.ps1 -BuildProfile debug   # builds workers, stages DLLs
cd app; npm ci; npm run build; cd ..
cargo test -p aural
```

## Running

```powershell
cd app
npx tauri dev          # dev build with hot reload
npx tauri build        # release build + NSIS installer in target/release/bundle/nsis
```

Set `AURAL_DATA_DIR` to a scratch folder to keep test runs away from your real models
and settings (both the data and settings folders move under it). Without it, Aural uses
`%LOCALAPPDATA%\com.aurathex.aural` and `%APPDATA%\com.aurathex.aural`.

The UI also runs in a normal browser (`npm run dev` in `app/`, open
`http://localhost:1420`) with demo data, which is handy for design work.

## Tests that need real hardware

Ignored by default; run them on a real Windows desktop:

```powershell
cargo test -p aural-audio -- --ignored                         # microphone
cargo test -p aural-platform -- --ignored --test-threads=1     # hotkey hook, autostart, real paste into a window
cargo test --release -p aural-stt-onnx -- --ignored            # needs AURAL_TEST_PARAKEET_DIR, AURAL_TEST_WAV
cargo test --release -p aural-stt-ggml -- --ignored            # needs AURAL_TEST_WHISPER_MODEL, AURAL_TEST_WAV
```

The insertion test briefly takes focus and uses the clipboard (your clipboard text is
restored afterwards).
