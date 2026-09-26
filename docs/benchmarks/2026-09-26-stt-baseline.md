# STT baseline benchmark (M0) — provisional

**Status: M0 gate OPEN (not passed).** The public-corpus results below meet every
decision rule and set **provisional** defaults. The gate closes only after the same
benchmark runs on the 30-clip personal dictation corpus (see "What is still open").

## Setup

| | |
|---|---|
| Machine | Laptop, Intel Core Ultra 9 185H, 15.4 GB RAM usable, on AC power |
| GPU | NVIDIA GeForce RTX 4070 **Laptop** GPU, 8 GB, driver 617.14 (Intel Arc iGPU present, not used) |
| OS | Windows 11 Home 26200 |
| Corpus | LibriSpeech test-clean, 100 clips (every 26th utterance, all 40 speakers), 13.3 min, 2,111 reference words — built by `tools/bench/scripts/prepare-librispeech.ps1` |
| Runner | `tools/bench/scripts/run-matrix.ps1`, 1 warm-up clip, runs sequential |
| Whisper | whisper.cpp 1.8.3 via whisper-rs 0.16.0 / whisper-rs-sys 0.15.0, 8 threads |
| Parakeet | transcribe-rs 0.3.11, ONNX Runtime 1.24.2 (ort 2.0.0-rc.12), CPU execution provider |
| Toolchains | Rust 1.98.1. CPU and Vulkan builds: MSVC 14.51.36231 (VS 2026 Build Tools). Vulkan SDK 1.4.357.0. **CUDA build: CUDA 12.9.1 (nvcc V12.9.86) with MSVC 14.44.35207** via Ninja, `CMAKE_CUDA_ARCHITECTURES=89` — CUDA 12.9's front end crashes on MSVC 14.51 headers, and it has no MSBuild integration for VS 2026 |

WER is pooled over the corpus after lowercasing and removing punctuation. One word is
about 0.05 WER points, so differences of a few tenths are a handful of words. p50/p95 are
per-clip latency (clips average 8 s). RTF = processing time / audio time. Engine RAM
excludes the decoded corpus. GPU memory is the peak `nvidia-smi` reading during the run
minus the idle reading just before it.

## Results

### CPU

| engine | WER | p50 ms | p95 ms | RTF | load ms | RAM MB |
|---|---|---|---|---|---|---|
| Parakeet TDT 0.6B v2 int8 | 2.51% | 301 | 1387 | 0.056 | 1879 | 860 |
| Parakeet TDT 0.6B v2 fp32 | 2.32% | 371 | 1755 | 0.071 | 7151 | 2549 |
| Whisper base.en q8_0 | 4.88% | 866 | 1120 | 0.165 | 243 | 274 |
| Whisper small.en q8_0 | 3.13% | 2708 | 3096 | 0.522 | 473 | 562 |
| Whisper large-v3-turbo q5_0 | 2.37% | 15614 | 16759 | 2.951 | 947 | 962 |

### RTX 4070 Laptop GPU

| engine | backend | WER | p50 ms | p95 ms | RTF | load ms | GPU MB |
|---|---|---|---|---|---|---|---|
| Whisper base.en q8_0 | Vulkan | 4.88% | 70 | 163 | 0.013 | 546 | +366 |
| Whisper small.en q8_0 | Vulkan | 3.27% | 130 | 289 | 0.024 | 731 | +906 |
| Whisper large-v3-turbo q5_0 | Vulkan | 2.56% | 295 | 394 | 0.056 | 1079 | +1076 |
| Whisper base.en q8_0 | CUDA | 4.74% | 73 | 178 | 0.013 | 436 | +483 |
| Whisper small.en q8_0 | CUDA | 3.17% | 145 | 338 | 0.027 | 966 | +890 |
| Whisper large-v3-turbo q5_0 | CUDA | 2.46% | 321 | 416 | 0.060 | 1571 | +1289 |

## Decision rules (from the M0 plan) applied

| Rule | Result | Decision |
|---|---|---|
| CPU default: Parakeet int8 if its WER ≤ fp32 WER + 0.5 pt | 2.51% vs 2.32% (+0.19 pt ≈ 4 words) | **Parakeet int8** stays the default. It also uses a third of fp32's RAM and loads 4× faster. |
| NVIDIA default: lowest-WER option with p50 ≤ 500 ms | Literally Parakeet fp32 on CPU (2.32%, 371 ms); turbo on GPU is 2.46–2.56% at ~300 ms; Parakeet int8 is 2.51% at 301 ms | **Parakeet int8 (CPU) provisionally**, because the gaps are 1–4 words out of 2,111, well inside the noise of 100 clips. Re-decide on the personal corpus. v0.1 runs Whisper on the CPU only, so no GPU path ships yet. |
| Build a CUDA pack only if CUDA p50 is ≥ 30% better than Vulkan | CUDA is 4–12% **slower** than Vulkan on every model | **No CUDA pack.** Vulkan covers NVIDIA. |
| Weak-CPU fallback: smallest Whisper with WER ≤ 12% | base.en q8 = 4.88% | **Whisper base.en q8** |

Also measured:
- large-v3-turbo on the CPU is slower than real time (RTF 2.95, about 16 s per clip). It is only usable on a GPU.
- Every GPU configuration used under 1.3 GB of the 8 GB.

## What is still open (why the gate is not passed)

1. **Personal dictation corpus.** 30 clips in the owner's voice (`tools/bench/corpus/personal-prompts.txt`), recorded with `tools/bench/scripts/record-corpus.ps1`, then `run-matrix.ps1 -Backends cpu,vulkan,cuda`. LibriSpeech is read audiobook speech, and dictation has different vocabulary and pacing.
2. **S1/S2 go/no-go (pill focus and insertion).** These are the manual real-app checks in `docs/testing/manual.md`. The automated hook and Edit-control paste tests pass on an unlocked desktop.
3. **AMD GPUs:** no hardware available; they remain unverified.

When 1 and 2 pass, update this file's status line and the defaults table, and M0 is closed.
