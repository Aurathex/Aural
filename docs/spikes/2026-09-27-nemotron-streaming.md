# Spike: Nemotron Speech Streaming EN 0.6B

**Verdict: no-go for v0.2 sub-project A** (offline dictation). Runtime and license are
workable, but there is no benchmark win over the models A already ships. Its real
strength, cache-aware streaming, belongs to sub-project B (live text); B's design may
look at it again on those merits, with the owner's approval of the license below.

## Model

- `nvidia/nemotron-speech-streaming-en-0.6b`: 0.6B cache-aware streaming FastConformer
  encoder with an RNN-T decoder; English, punctuation and capitals; offline and
  streaming modes.
- Export tried: `handy-computer/nemotron-speech-streaming-en-0.6b-gguf` @
  `9789e0ebf77277911272f0d9a35e1646b5aa6004`, file
  `nemotron-speech-streaming-en-0.6b-Q8_0.gguf` (730 MB, SHA-256
  `90d8c897…a228ca110`), verified after download.

## Runtime

- **transcribe.cpp v0.2.4** (`handy-computer/transcribe.cpp`, commit
  `4807edaf210d0d7e8a6f7fb2a44b65966a2797f0`), MIT, C/C++ on ggml. Built from source on
  this PC with MSVC + Ninja, `-DTRANSCRIBE_VULKAN=ON -DGGML_VULKAN=ON` (Vulkan SDK
  1.4.357.0). It builds cleanly; the project also publishes Windows CPU+Vulkan and CUDA
  binaries.
- Used through its `transcribe-cli --batch` (offline mode). Integrating it into Aural
  would mean a third worker family (next to ONNX Runtime and whisper.cpp) with its own
  ggml copy.

## Benchmark (this PC: Core Ultra 9 185H, RTX 4070 Laptop, driver 617.14)

LibriSpeech-100 (the same 100 clips as M0 and `docs/benchmarks/2026-09-27-v0.2-a.md`),
one warm-up, pooled WER with Aural's scorer (`aural-eval`). Latency is the time between
consecutive results from the CLI (model loaded once), so it matches the "text appears
after you stop" measure used elsewhere.

| runs on | WER | p50 ms | p95 ms | load ms | peak RAM MB |
|---|---|---|---|---|---|
| processor | 2.51% | 538 | 1,698 | 1,052 | 1,365 |
| graphics card (Vulkan) | 2.51% | 247 | 470 | 1,024 | 768 |

For comparison (same clips, Aural's catalog): Parakeet TDT v2 on the processor 2.51% /
279 ms; Whisper large-v3-turbo on Vulkan 2.56% / 295 ms.

- **Accuracy:** identical to Parakeet v2 (the margin of error on 2,111 words is about
  ±0.7 points).
- **Speed:** about half Parakeet's speed on the processor; on the graphics card about
  50 ms faster than turbo, with both well under the 500 ms dictation budget.

So for A's offline dictation it adds a runtime without making anything better for the
user.

## License (read in full)

NVIDIA Open Model License Agreement (version of 24 Oct 2025):

- Commercial use allowed; derivative models allowed; NVIDIA claims no rights in outputs.
- Redistributing the model requires shipping a copy of the agreement and a "Notice" file
  with "Licensed by NVIDIA Corporation under the NVIDIA Open Model License". (Aural would
  download it on request rather than redistribute it, but the attribution would still be
  shown.)
- The license terminates automatically if you sue over the model's IP, or bypass its
  "guardrails"; NVIDIA may update the agreement and you must comply or stop using it.
- Use must follow NVIDIA's Trustworthy AI terms; the user indemnifies NVIDIA; Delaware
  law, Santa Clara courts; export rules apply.

These terms are more restrictive and changeable than the MIT / CC BY / Apache models in
the catalog. Using this model would need the owner's explicit approval.

## If revisited in sub-project B

- Streaming in transcribe.cpp: `--stream-chunk-ms`, right-context menu
  `--stream-att-right` (the model supports several latency settings).
- Measure first-word latency and word stability while speaking, not just final WER.
- Compare against transcribe-rs's Moonshine streaming model (MIT) before adding a third
  runtime.

No code from this spike is merged; the build and harness lived outside the repository.
