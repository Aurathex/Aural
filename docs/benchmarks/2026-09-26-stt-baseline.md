# STT baseline benchmark (M0)

**Status: M0 gate OPEN.** The public-corpus benchmark below is complete and sets the
**provisional** model recommendations. The gate closes only after the same benchmark
runs on the owner's 30-clip personal dictation corpus (see "What is still open"). That
step has not been done.

Run: 2026-09-26, 20:10–20:47, sequential, one model at a time.

## Hardware and software

| | |
|---|---|
| CPU | Intel Core Ultra 9 185H (16 cores, 22 threads) |
| GPU | NVIDIA GeForce RTX 4070 **Laptop** GPU, 8 GB (8188 MiB), driver 617.14. The Intel Arc iGPU is present but not used. |
| RAM | 15.4 GB usable |
| OS / power | Windows 11 Home 26200, on AC power |
| Whisper | whisper.cpp 1.8.3 via whisper-rs 0.16.0 / whisper-rs-sys 0.15.0, 8 threads |
| Parakeet | transcribe-rs 0.3.11, ONNX Runtime 1.24.2 (ort 2.0.0-rc.12), CPU execution provider, thread pool sized by the engine |
| Toolchains | Rust 1.98.1. CPU and Vulkan builds: MSVC 14.51.36231 (VS 2026 Build Tools), Vulkan SDK 1.4.357.0. CUDA build: CUDA 12.9.1 (nvcc V12.9.86) with MSVC 14.44.35207 via Ninja, `CMAKE_CUDA_ARCHITECTURES=89` (see `tools/bench/README.md`) |
| Models | Pinned Hugging Face commits with SHA-256 (`manifests/catalog.v1.json`, bench-only turbo: `ggerganov/whisper.cpp@5359861`, sha256 `394221709c…a55ffa7e2`) |

## Corpus and method

- **Corpus:** LibriSpeech test-clean (CC BY 4.0), 100 clips: every 26th of the 2,620
  utterances, which covers all 40 speakers. 13.3 min of audio, 2,111 reference words,
  about 8 s per clip. Built by `tools/bench/scripts/prepare-librispeech.ps1`.
- **Runner:** `tools/bench/scripts/run-matrix.ps1`, with one warm-up clip per model.
- **WER** is pooled over the corpus after lowercasing and removing punctuation; numbers
  are not normalized. One word is 0.047 points. At about 2.5% WER the 95% margin is
  about ±0.7 points, so gaps smaller than that are not meaningful.
- **p50 / p95** are per-clip transcription latency. **RTF** is processing time divided
  by audio time (below 1 is faster than real time). **Load** is time to load the model.
- **RAM** is the engine's peak working set, not counting the decoded corpus.
- **GPU memory** is the peak `nvidia-smi` reading during the run minus the idle reading
  just before it.
- The backend was verified from whisper.cpp's own initialization log. A run that falls
  back to the CPU is rejected rather than reported.

## Results

| Model | Weights | Backend | WER | p50 ms | p95 ms | RTF | Load ms | RAM MB | GPU MB |
|---|---|---|---|---|---|---|---|---|---|
| Parakeet TDT 0.6B v2 | int8 (ONNX) | CPU | 2.51% | 257 | 1359 | 0.049 | 2060 | 861 | — |
| Parakeet TDT 0.6B v2 | fp32 (ONNX) | CPU | 2.32% | 363 | 1728 | 0.068 | 8031 | 2550 | — |
| Whisper base.en | q8_0 | CPU | 4.88% | 819 | 1006 | 0.155 | 256 | 275 | — |
| Whisper small.en | q8_0 | CPU | 3.13% | 2740 | 3416 | 0.559 | 624 | 562 | — |
| Whisper large-v3-turbo | q5_0 | CPU | 2.37% | 15589 | 16337 | 3.027 | 1381 | 962 | — |
| Whisper base.en | q8_0 | Vulkan (RTX 4070) | 4.88% | 67 | 172 | 0.013 | 869 | 183 | +369 |
| Whisper small.en | q8_0 | Vulkan (RTX 4070) | 3.27% | 131 | 298 | 0.024 | 1182 | 357 | +911 |
| Whisper large-v3-turbo | q5_0 | Vulkan (RTX 4070) | 2.56% | 292 | 402 | 0.056 | 1208 | 616 | +1076 |
| Whisper base.en | q8_0 | CUDA (RTX 4070) | 4.74% | 71 | 162 | 0.013 | 392 | 294 | +475 |
| Whisper small.en | q8_0 | CUDA (RTX 4070) | 3.17% | 146 | 317 | 0.026 | 408 | 295 | +763 |
| Whisper large-v3-turbo | q5_0 | CUDA (RTX 4070) | 2.46% | 307 | 400 | 0.059 | 684 | 300 | +1163 |

The large-v3-turbo CUDA row's GPU memory comes from a repeat run at 20:49 (same
WER, p50 305 ms), because the matrix's sampler stopped before its last readings were
written. An earlier run the same day (13:31–13:44, before the GPU-detection fix) gave
the same WER for every model and latencies within about 10%.

## Provisional recommendations

These follow the decision rules in the M0 plan.

| Rule | Measured | Recommendation |
|---|---|---|
| CPU default: Parakeet int8 if its WER ≤ fp32 WER + 0.5 pt | 2.51% vs 2.32% (+0.19 pt, about 4 words) | **Parakeet TDT 0.6B v2 int8 on CPU is the default for every PC.** Fastest accurate option (p50 257 ms, RTF 0.049), 861 MB RAM, 2 s load. fp32 is not measurably more accurate, uses 3× the RAM and loads 4× slower. |
| NVIDIA default: lowest-WER option with p50 ≤ 500 ms | Parakeet int8 (CPU) 2.51% / 257 ms; turbo 2.46–2.56% / 292–307 ms on GPU; Parakeet fp32 (CPU) 2.32% / 363 ms | **Parakeet int8 on CPU for v0.1, including NVIDIA PCs.** The three top options are within 0.25 pt of each other (about 5 words), inside the margin. v0.1 doesn't ship a GPU path. **Whisper large-v3-turbo on the GPU** is the candidate for a later GPU option, since it matches Parakeet's accuracy with the CPU left free, in about 1.1 GB of GPU memory. |
| Build a CUDA pack only if CUDA p50 is ≥ 30% better than Vulkan | CUDA is 2–11% slower on p50 and about the same on p95 | **No CUDA pack.** Vulkan covers NVIDIA and doesn't need the CUDA runtime (roughly 0.6 GB). |
| Weak-CPU fallback: smallest Whisper with WER ≤ 12% | base.en q8_0: 4.88%, 275 MB RAM | **Whisper base.en q8_0** for PCs that are short on RAM or CPU. |

Not recommended:
- **Whisper small.en on CPU** takes 2.7 s per clip. On a GPU it is fast but no more
  accurate than Parakeet.
- **large-v3-turbo on CPU** is three times slower than real time.

## What is still open (why the gate is not passed)

1. **Personal dictation corpus.** 30 clips in the owner's voice
   (`tools/bench/corpus/personal-prompts.txt`), recorded with
   `tools/bench/scripts/record-corpus.ps1`, then
   `run-matrix.ps1 -Backends cpu,vulkan,cuda`. LibriSpeech is read audiobook speech;
   dictation has different vocabulary, pacing and microphones.
2. **AMD GPUs:** no hardware was available; still unverified.

When the personal corpus has been run, update this file's status line and the
recommendations, and M0 is closed.
