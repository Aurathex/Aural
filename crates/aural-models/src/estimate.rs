//! Estimated numbers for a variant that isn't downloaded yet: the catalog's reference
//! numbers scaled by how fast a small probe model ran on this PC versus the reference PC.

use crate::catalog::{ModelEntry, Runtime, Variant};
use crate::recommend::HardwareProfile;
use aural_engines::Backend;
use aural_eval::metrics::RunMetrics;
use aural_stt_protocol::bench::{Stability, VariantResult};
use serde::{Deserialize, Serialize};

/// Memory estimates get this much headroom over the reference numbers.
const MEMORY_MARGIN: f64 = 1.2;

/// A probe model measured on this PC, for one runtime and backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calibration {
    pub runtime: Runtime,
    pub backend: Backend,
    /// The probe's p50 on this PC.
    pub probe_p50_ms: u64,
    /// The probe's p50 on the reference PC (catalog).
    pub probe_ref_p50_ms: u64,
}

/// `None` when the variant has no reference numbers or this PC has no calibration for
/// its runtime and backend.
pub fn estimate(
    model: &ModelEntry,
    v: &Variant,
    cal: &[Calibration],
    _hw: &HardwareProfile,
) -> Option<VariantResult> {
    let reference = v.reference.as_ref()?;
    let c = cal
        .iter()
        .find(|c| c.runtime == model.runtime && c.backend == v.backend)?;
    let scale = c.probe_p50_ms as f64 / c.probe_ref_p50_ms.max(1) as f64;
    let p50 = (reference.p50_ms as f64 * scale).round() as u64;
    let margin = |mb: u64| (mb as f64 * MEMORY_MARGIN).round() as u64;
    Some(VariantResult {
        variant: model.variant_id(v.backend),
        metrics: Some(RunMetrics {
            wer: reference.clips_wer.unwrap_or(reference.wer),
            // Accuracy comes from the reference run, not from this PC's clips.
            words: 0,
            p50_ms: p50,
            p95_ms: p50,
            rtf: reference.rtf * scale,
        }),
        load_ms: (reference.load_ms as f64 * scale).round() as u64,
        ram_mb: margin(reference.ram_mb.max(v.min_ram_mb)),
        vram_mb: (v.backend != Backend::Cpu).then(|| margin(reference.vram_mb)),
        spread: 1.0,
        passes: 0,
        stability: Stability::Stable,
        measured: false,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Reference;
    use crate::test_fixtures::entry;
    use aural_engines::Engine;

    fn variant(backend: Backend, p50: u64) -> Variant {
        Variant {
            backend,
            min_ram_mb: 1_000,
            min_vram_mb: if backend == Backend::Cpu { 0 } else { 1_000 },
            reference: Some(Reference {
                wer: 0.03,
                clips_wer: None,
                p50_ms: p50,
                rtf: 0.1,
                load_ms: 900,
                ram_mb: 1_000,
                vram_mb: if backend == Backend::Cpu { 0 } else { 800 },
            }),
        }
    }

    fn cal(backend: Backend, measured: u64, reference: u64) -> Calibration {
        Calibration {
            runtime: Runtime::Ggml,
            backend,
            probe_p50_ms: measured,
            probe_ref_p50_ms: reference,
        }
    }

    #[test]
    fn estimate_scales_reference_by_probe_calibration() {
        let m = entry("whisper-small.en-q8", Engine::Whisper, false);
        let v = variant(Backend::Vulkan, 400);
        let r = estimate(
            &m,
            &v,
            &[cal(Backend::Vulkan, 150, 100)],
            &HardwareProfile::default(),
        )
        .unwrap();
        assert!(!r.measured);
        assert_eq!(r.variant, "whisper-small.en-q8@vulkan");
        let e = r.metrics.unwrap();
        assert_eq!(e.p50_ms, 600);
        assert!((e.rtf - 0.15).abs() < 1e-9);
        assert_eq!(e.wer, 0.03);
        assert_eq!(r.ram_mb, 1_200);
        assert_eq!(r.vram_mb, Some(960));
        assert_eq!(r.stability, Stability::Stable);
    }

    #[test]
    fn accuracy_comes_from_the_built_in_clips_when_known() {
        // Measured results use the built-in clips, so estimates must too, or the two
        // can't be compared (LibriSpeech-100 is harder: 2.5% vs 0.5% for Parakeet).
        let m = entry("whisper-small.en-q8", Engine::Whisper, false);
        let mut v = variant(Backend::Cpu, 400);
        v.reference.as_mut().unwrap().clips_wer = Some(0.0101);
        let r = estimate(
            &m,
            &v,
            &[cal(Backend::Cpu, 100, 100)],
            &HardwareProfile::default(),
        )
        .unwrap();
        assert_eq!(r.metrics.unwrap().wer, 0.0101);
    }

    #[test]
    fn no_calibration_for_a_backend_means_no_estimate() {
        let m = entry("whisper-small.en-q8", Engine::Whisper, false);
        let hw = HardwareProfile::default();
        let only_cpu = [cal(Backend::Cpu, 150, 100)];
        assert!(estimate(&m, &variant(Backend::Vulkan, 400), &only_cpu, &hw).is_none());
        // A calibration from the other runtime doesn't count either.
        let onnx = [Calibration {
            runtime: Runtime::Onnx,
            ..cal(Backend::Vulkan, 150, 100)
        }];
        assert!(estimate(&m, &variant(Backend::Vulkan, 400), &onnx, &hw).is_none());
    }

    #[test]
    fn no_reference_numbers_means_no_estimate() {
        let m = entry("whisper-small.en-q8", Engine::Whisper, false);
        let mut v = variant(Backend::Cpu, 400);
        v.reference = None;
        let r = estimate(
            &m,
            &v,
            &[cal(Backend::Cpu, 1, 1)],
            &HardwareProfile::default(),
        );
        assert!(r.is_none());
    }

    #[test]
    fn a_processor_estimate_has_no_gpu_memory() {
        let m = entry("whisper-small.en-q8", Engine::Whisper, false);
        let r = estimate(
            &m,
            &variant(Backend::Cpu, 400),
            &[cal(Backend::Cpu, 100, 100)],
            &HardwareProfile::default(),
        )
        .unwrap();
        assert_eq!(r.vram_mb, None);
        assert_eq!(r.metrics.map(|m| m.p50_ms), Some(400));
        let _: Option<RunMetrics> = r.metrics;
    }
}
