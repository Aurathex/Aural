# aural-bench

Developer tool that measures local STT engines on a WAV corpus: word error rate, latency
(p50/p95), real-time factor, model load time and peak RAM. It is not shipped with Aural.

## Build prerequisites (Windows)

- Visual Studio 2022 Build Tools (MSVC + Windows SDK), Rust per `rust-toolchain.toml`
- **Parakeet (`--features onnx`)**: nothing else. ONNX Runtime binaries are fetched by the
  `ort` crate at build time.
- **Whisper (`--features ggml`)**: CMake and LLVM (for libclang, used by bindgen):
  ```powershell
  winget install Kitware.CMake
  winget install LLVM.LLVM
  $env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
  ```
  `--features vulkan` also needs the Vulkan SDK; `--features cuda` needs the CUDA Toolkit 12.x.

`onnx` and `ggml` cannot be enabled together (ONNX Runtime and whisper.cpp must not link
into one binary), so build each engine separately.

The repo's `.cargo/config.toml` sets `CL=/O2`. Without it whisper.cpp compiles unoptimized
on MSVC and runs about 6x slower. If `CL` is already set in your shell, that value wins,
so unset it before building.

## Corpus

Keep audio **outside the repo** (for example `%USERPROFILE%\aural-bench-corpus`). Write a
`corpus.tsv` next to it, with one tab-separated line per clip:

```
# id	wav (relative to this file, or absolute)	reference transcript
c01	wav/c01.wav	Please send the quarterly report to Maria by Friday.
```

WAV files can be any sample rate or channel count; they are converted to 16 kHz mono.

## Run

```powershell
cargo run --release -p aural-bench --features onnx -- `
  --engine parakeet --model "$env:LOCALAPPDATA\Aural\models\parakeet-tdt-0.6b-v2-int8" `
  --corpus corpus.tsv --out parakeet-int8.json

cargo run --release -p aural-bench --features ggml -- `
  --engine whisper --model "$env:LOCALAPPDATA\Aural\models\whisper\ggml-small.en-q8_0.bin" `
  --corpus corpus.tsv --out whisper-small.json --threads 6
```

Each run prints a markdown table row and writes per-clip JSON. Read VRAM from `nvidia-smi`
or Task Manager during GPU runs.

Notes:

- `--threads` applies to Whisper only. transcribe-rs sizes Parakeet's ONNX Runtime thread
  pool itself (about one thread per physical core), so Parakeet rows show `engine-default`.
- `backend` is the backend whisper.cpp actually initialized. A run that asked for Vulkan
  or CUDA but fell back to the CPU fails instead of reporting CPU numbers. Set
  `AURAL_BENCH_VERBOSE=1` to see the native init log.
- `engine RAM MB` is peak working set minus the working set after the corpus is decoded,
  so corpus audio isn't counted as model memory. `load ms` depends on whether the model
  file is already in the OS file cache; run twice and report the warm number.
- WER normalization lowercases and strips punctuation but does not normalize numbers
  ("ten" vs "10"). Both engines are scored the same way.
