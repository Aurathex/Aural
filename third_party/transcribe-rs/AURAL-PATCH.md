# transcribe-rs 0.3.11, patched for Aural

Upstream: https://crates.io/crates/transcribe-rs (repository
https://github.com/cjpais/transcribe-rs), MIT License — see LICENSE (Copyright (c) 2025
Ilya Stupakov), kept unchanged. Source copied unmodified from the published 0.3.11 crate
(src, build.rs, LICENSE, README), except:

- `Cargo.toml`: example, test and dev-dependency sections removed (those files are not
  vendored).
- `src/accel.rs`: `set_directml_device` / `get_directml_device` (new).
- `src/onnx/session.rs`: DirectML uses the chosen device id when one is set.
- `src/onnx/moonshine/streaming.rs`: a public live-stream API (`start_stream`,
  `push_audio`, `partial_text`, `finish_stream`, `LiveStream`) over the existing private
  incremental frontend/encoder, and `tokenizer.json` support (the published ONNX exports
  ship `tokenizer.json`, not `tokenizer.bin`).
- `src/onnx/moonshine/model.rs`: `MoonshineTokenizer` visible inside the crate (reused by
  the streaming model).
- `src/onnx/moonshine/mod.rs`: exports `LiveStream`.

Why (DirectML): on hybrid laptops DXGI adapter 0 is often the built-in graphics; Aural needs to run
DirectML on the separate graphics card. Drop this copy once transcribe-rs can choose the
DirectML device itself.

Why (Moonshine Streaming): live text needs audio encoded as it arrives; upstream only
exposes whole-recording transcription for the streaming model.
