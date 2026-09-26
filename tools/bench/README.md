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

  CUDA 12.9 with **VS 2026 Build Tools**: CUDA 12.9 has no MSBuild integration for VS 2026,
  and its front end crashes on the MSVC 14.51 headers. Add the MSVC v14.44 (VS 2022) toolset
  with the Visual Studio Installer, then build with Ninja from that toolset's environment:
  ```powershell
  cmd /c "`"$vs\VC\Auxiliary\Build\vcvars64.bat`" -vcvars_ver=14.44 && set"   # load into the shell
  $env:CMAKE_GENERATOR = 'Ninja'            # ninja ships with the Build Tools CMake component
  $env:CMAKE_CUDA_ARCHITECTURES = '89'      # RTX 40 series; omit to build every architecture
  ```

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

## M0 benchmark procedure

The M0 gate compares engines on two corpora, with the same scripts on every machine.
Audio and results stay outside the repo.

1. **Public corpus (provisional defaults).** Download LibriSpeech test-clean from
   <https://www.openslr.org/12> (MD5 `32fa31d27d2e1cad72775fee3f4849a9`), extract it, then:
   ```powershell
   ./tools/bench/scripts/prepare-librispeech.ps1 -Source <folder with LibriSpeech\test-clean> `
     -Out $env:USERPROFILE\aural-bench-corpus\librispeech-100
   ```
   This picks 100 clips spread over all 40 speakers, the same ones every time.
2. **Personal corpus (required to close M0).** Record the 30 prompts in
   `tools/bench/corpus/personal-prompts.txt` in your own voice:
   ```powershell
   ./tools/bench/scripts/record-corpus.ps1
   ```
   Press Enter, wait for **SPEAK NOW**, read the line, press Enter. If you said
   something different, fix that line's reference in `corpus.tsv`. Re-record single
   clips with `-Only p07,p12`. To import clips recorded elsewhere, put the WAVs next to
   a `corpus.tsv` in the format above.
3. **Run the matrix** on each corpus (Vulkan and CUDA need their SDKs, see above):
   ```powershell
   ./tools/bench/scripts/run-matrix.ps1 -Corpus <corpus.tsv> -Out <results folder> `
     -Backends cpu,vulkan,cuda
   ```
   If the Vulkan build fails with MSVC error C1083 (`Cannot open compiler generated file: ''`),
   the checkout path is too long for the nested shader build: add `-TargetDir C:\t\aural`
   (any short folder). Each run appends a row to `<results folder>\table.md` and writes per-clip JSON.
4. Copy the tables into `docs/benchmarks/` and apply the decision rules written there.
