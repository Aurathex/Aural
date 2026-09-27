# transcribe-rs 0.3.11, patched for Aural

Upstream: https://crates.io/crates/transcribe-rs (MIT License, see LICENSE; copyright
the transcribe-rs authors). Source copied unmodified from the published 0.3.11 crate
(src, build.rs, LICENSE, README), except:

- `Cargo.toml`: example, test and dev-dependency sections removed (those files are not
  vendored).
- `src/accel.rs`: `set_directml_device` / `get_directml_device` (new).
- `src/onnx/session.rs`: DirectML uses the chosen device id when one is set.

Why: on hybrid laptops DXGI adapter 0 is often the built-in graphics; Aural needs to run
DirectML on the separate graphics card. Drop this copy once transcribe-rs can choose the
DirectML device itself.
