//! The five labels the Models page shows, computed from measured or estimated results
//! and the hardware. One pure function; no universal "best" model is hard-coded.

use crate::catalog::{ModelEntry, Variant};
use crate::recommend::HardwareProfile;
use aural_engines::Backend;
use aural_eval::wer::margin_95;
use aural_platform::gpu::primary_discrete;
use aural_stt_protocol::bench::{Stability, VariantResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Slower than this after you stop speaking feels like waiting.
pub const DICTATION_BUDGET_MS: u64 = 500;
/// To be recommended, a model may use at most this share of free RAM…
pub const RAM_HEADROOM: f64 = 0.5;
/// …and of the graphics card's memory.
pub const VRAM_HEADROOM: f64 = 0.7;
/// Streaming needs to keep up comfortably while you talk.
pub const LIVE_TEXT_MAX_RTF: f64 = 0.3;
/// "Fastest" only goes to a model at most this much less accurate than the best.
pub const FASTEST_MAX_WER_RATIO: f64 = 1.5;
/// Measured accuracy this bad is never usable…
pub const MAX_WER: f64 = 0.30;
/// …nor is one this far above the model's reference: WER > ratio × reference + slack.
pub const WER_DRIFT_RATIO: f64 = 2.0;
pub const WER_DRIFT_SLACK: f64 = 0.03;
/// An estimate must be this much faster than a measured variant to beat it.
pub const ESTIMATE_SPEED_EDGE: f64 = 0.8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "label", rename_all = "snake_case")]
pub enum Label {
    Recommended,
    MostAccurate,
    Fastest,
    LiveText,
    WontWorkWell { reason: Reason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    NoGpu,
    GpuFailed {
        detail: String,
    },
    NotEnoughMemory {
        need_mb: u64,
    },
    NotEnoughGpuMemory {
        need_mb: u64,
    },
    TooSlow,
    /// Far more mistakes than this model makes elsewhere: something is wrong with it
    /// on this PC (e.g. a graphics driver that produces garbage).
    TooManyMistakes,
    Unstable {
        detail: String,
    },
}

/// Labels per variant id. `words` is the number of reference words the WERs were
/// measured over (sets the margin of error).
pub fn labels(
    results: &[(ModelEntry, Variant, VariantResult)],
    hw: &HardwareProfile,
    words: usize,
) -> BTreeMap<String, Vec<Label>> {
    let mut out: BTreeMap<String, Vec<Label>> = BTreeMap::new();
    let mut suitable: Vec<Candidate> = Vec::new();
    for (model, variant, result) in results {
        let id = model.variant_id(variant.backend);
        let mut labels = Vec::new();
        match unsuitable(variant, result, hw) {
            Some(reason) => labels.push(Label::WontWorkWell { reason }),
            None => {
                if let Some(m) = &result.metrics {
                    suitable.push(Candidate {
                        id: id.clone(),
                        wer: m.wer,
                        p50: m.p50_ms,
                        ram: result.ram_mb,
                        measured: result.measured,
                        roomy: roomy(variant, result, hw),
                    });
                    if model.streaming && m.rtf <= LIVE_TEXT_MAX_RTF {
                        labels.push(Label::LiveText);
                    }
                }
            }
        }
        out.insert(id, labels);
    }

    let Some(best_wer) = suitable.iter().map(|c| c.wer).min_by(f64::total_cmp) else {
        return out;
    };
    // At least 3 errors' worth: with 0 errors the usual margin would be 0, and a lucky
    // perfect score would shut out equally good models (rule of three).
    let margin = margin_95(best_wer, words).max(3.0 / words.max(1) as f64);
    // Faster first, then smaller.
    suitable.sort_by(|a, b| a.p50.cmp(&b.p50).then(a.ram.cmp(&b.ram)));
    let accurate: Vec<&Candidate> = suitable
        .iter()
        .filter(|c| c.wer <= best_wer + margin)
        .collect();
    let fastest = suitable
        .iter()
        .find(|c| c.wer <= (best_wer * FASTEST_MAX_WER_RATIO).max(best_wer + margin));

    let in_budget: Vec<&Candidate> = accurate
        .iter()
        .copied()
        .filter(|c| c.p50 <= DICTATION_BUDGET_MS && c.roomy)
        .collect();
    let recommended = pick_recommended(&in_budget, margin).or(fastest);

    let mut add = |c: Option<&Candidate>, label: Label| {
        if let Some(c) = c {
            if let Some(v) = out.get_mut(&c.id) {
                v.push(label);
            }
        }
    };
    add(recommended, Label::Recommended);
    add(accurate.first().copied(), Label::MostAccurate);
    add(fastest, Label::Fastest);
    for v in out.values_mut() {
        v.sort_by_key(order);
    }
    out
}

struct Candidate {
    id: String,
    wer: f64,
    p50: u64,
    ram: u64,
    measured: bool,
    /// Leaves enough memory free to be recommended.
    roomy: bool,
}

/// At most half the free memory and 70% of the graphics card's.
fn roomy(v: &Variant, r: &VariantResult, hw: &HardwareProfile) -> bool {
    let ram_ok = need(r.ram_mb, v.min_ram_mb) as f64 <= hw.free_ram_mb as f64 * RAM_HEADROOM;
    let vram_ok = v.backend == Backend::Cpu
        || primary_discrete(&hw.gpus).is_some_and(|g| {
            need(r.vram_mb.unwrap_or(0), v.min_vram_mb) as f64 <= g.vram_mb as f64 * VRAM_HEADROOM
        });
    ram_ok && vram_ok
}

/// Measured results are trusted over estimates: an estimate wins only when it is more
/// accurate by more than the margin of error and at least 20% faster.
fn pick_recommended<'a>(sorted: &[&'a Candidate], margin: f64) -> Option<&'a Candidate> {
    let measured = sorted.iter().copied().find(|c| c.measured);
    let estimated = sorted.iter().copied().find(|c| !c.measured);
    match (measured, estimated) {
        (Some(m), Some(e)) => {
            let clearly_better =
                e.wer + margin < m.wer && (e.p50 as f64) < m.p50 as f64 * ESTIMATE_SPEED_EDGE;
            Some(if clearly_better { e } else { m })
        }
        (m, e) => m.or(e),
    }
}

fn unsuitable(v: &Variant, r: &VariantResult, hw: &HardwareProfile) -> Option<Reason> {
    let gpu = primary_discrete(&hw.gpus);
    let on_gpu = v.backend != Backend::Cpu;
    if on_gpu && gpu.is_none() {
        return Some(Reason::NoGpu);
    }
    if let Some(error) = &r.error {
        // "Didn't work on the graphics card" only when nothing ran there; a run that
        // started and then timed out or crashed is unstable.
        return Some(if on_gpu && r.metrics.is_none() {
            Reason::GpuFailed {
                detail: error.clone(),
            }
        } else {
            Reason::Unstable {
                detail: unstable_detail(r).unwrap_or_else(|| error.clone()),
            }
        });
    }
    if let Some(detail) = unstable_detail(r) {
        return Some(Reason::Unstable { detail });
    }
    // Won't fit at all. (Headroom only decides whether it can be recommended.)
    let need_ram = need(r.ram_mb, v.min_ram_mb);
    if need_ram > hw.free_ram_mb {
        return Some(Reason::NotEnoughMemory { need_mb: need_ram });
    }
    if let Some(gpu) = gpu.filter(|_| on_gpu) {
        let need_vram = need(r.vram_mb.unwrap_or(0), v.min_vram_mb);
        if need_vram > gpu.vram_mb {
            return Some(Reason::NotEnoughGpuMemory { need_mb: need_vram });
        }
    }
    if let (true, Some(m)) = (r.measured, &r.metrics) {
        let expected = v.reference.as_ref().map_or(f64::INFINITY, |re| {
            re.wer * WER_DRIFT_RATIO + WER_DRIFT_SLACK
        });
        if m.wer > MAX_WER || m.wer > expected {
            return Some(Reason::TooManyMistakes);
        }
    }
    if r.metrics.is_some_and(|m| m.rtf > 1.0) {
        return Some(Reason::TooSlow);
    }
    None
}

/// Memory a model needs: what it used when measured (or its estimate), else the
/// catalog's conservative install minimum.
fn need(used_mb: u64, minimum_mb: u64) -> u64 {
    if used_mb > 0 {
        used_mb
    } else {
        minimum_mb
    }
}

fn unstable_detail(r: &VariantResult) -> Option<String> {
    match &r.stability {
        Stability::Unstable { reason } => Some(reason.clone()),
        Stability::Stable => None,
    }
}

fn order(l: &Label) -> u8 {
    match l {
        Label::Recommended => 0,
        Label::MostAccurate => 1,
        Label::Fastest => 2,
        Label::LiveText => 3,
        Label::WontWorkWell { .. } => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Reference;
    use crate::test_fixtures::entry;
    use aural_engines::{Backend, Engine};
    use aural_eval::metrics::RunMetrics;
    use aural_platform::gpu::{GpuInfo, Vendor};
    use aural_stt_protocol::bench::Stability;

    const WORDS: usize = 2111;

    type Row = (ModelEntry, Variant, VariantResult);

    fn gpu(name: &str, vendor: Vendor, vram_mb: u64, integrated: bool) -> GpuInfo {
        GpuInfo {
            name: name.into(),
            vendor,
            vram_mb,
            integrated,
            driver: "1.0".into(),
            luid: 0,
        }
    }

    fn hw(total: u64, free: u64, cores: usize, gpus: Vec<GpuInfo>) -> HardwareProfile {
        HardwareProfile {
            cpu_name: "test".into(),
            total_ram_mb: total,
            free_ram_mb: free,
            logical_cores: cores,
            physical_cores: cores / 2,
            avx2: true,
            avx512: false,
            gpus,
        }
    }

    struct R {
        id: &'static str,
        engine: Engine,
        streaming: bool,
        backend: Backend,
        wer: f64,
        p50: u64,
        rtf: f64,
        ram: u64,
        vram: u64,
        measured: bool,
    }

    fn r(id: &'static str, backend: Backend, wer: f64, p50: u64, rtf: f64, ram: u64) -> R {
        R {
            id,
            engine: if id.starts_with("whisper") {
                Engine::Whisper
            } else if id.starts_with("moonshine") {
                Engine::Moonshine
            } else {
                Engine::Parakeet
            },
            streaming: false,
            backend,
            wer,
            p50,
            rtf,
            ram,
            vram: if backend == Backend::Cpu { 0 } else { 900 },
            measured: true,
        }
    }

    impl R {
        fn est(mut self) -> R {
            self.measured = false;
            self
        }
        fn vram(mut self, mb: u64) -> R {
            self.vram = mb;
            self
        }
        fn streaming(mut self) -> R {
            self.streaming = true;
            self
        }
        fn row(self) -> Row {
            let m = entry(self.id, self.engine, self.streaming);
            let v = Variant {
                backend: self.backend,
                min_ram_mb: self.ram,
                min_vram_mb: self.vram,
                reference: Some(Reference {
                    wer: self.wer,
                    clips_wer: None,
                    p50_ms: self.p50,
                    rtf: self.rtf,
                    load_ms: 1_000,
                    ram_mb: self.ram,
                    vram_mb: self.vram,
                }),
            };
            let res = VariantResult {
                variant: m.variant_id(self.backend),
                metrics: Some(RunMetrics {
                    wer: self.wer,
                    words: WORDS,
                    p50_ms: self.p50,
                    p95_ms: self.p50 * 3 / 2,
                    rtf: self.rtf,
                }),
                load_ms: 1_000,
                ram_mb: self.ram,
                vram_mb: (self.backend != Backend::Cpu).then_some(self.vram),
                spread: 1.5,
                passes: 3,
                stability: Stability::Stable,
                measured: self.measured,
                error: None,
            };
            (m, v, res)
        }
    }

    const PK: &str = "parakeet-tdt-0.6b-v2-int8";
    const SMALL: &str = "whisper-small.en-q8";
    const BASE: &str = "whisper-base.en-q8";
    const TURBO: &str = "whisper-large-v3-turbo-q5";
    const MOON: &str = "moonshine-streaming";

    fn cpu_8gb_laptop() -> (HardwareProfile, Vec<Row>) {
        let hw = hw(
            8_000,
            4_000,
            8,
            vec![gpu("Intel(R) UHD Graphics", Vendor::Intel, 128, true)],
        );
        let rows = vec![
            r(PK, Backend::Cpu, 0.0251, 420, 0.08, 861).row(),
            r(PK, Backend::DirectMl, 0.0251, 300, 0.05, 800).est().row(),
            r(SMALL, Backend::Cpu, 0.0313, 3_900, 0.8, 562).row(),
            r(SMALL, Backend::Vulkan, 0.0313, 600, 0.1, 600).est().row(),
            r(BASE, Backend::Cpu, 0.0488, 1_200, 0.23, 275).row(),
        ];
        (hw, rows)
    }

    /// M0 reference numbers (RTX 4070 laptop, Core Ultra 9 185H).
    fn rtx4070_laptop() -> (HardwareProfile, Vec<Row>) {
        let hw = hw(
            15_770,
            8_000,
            22,
            vec![
                gpu("Intel(R) Arc(TM) Graphics", Vendor::Intel, 2_048, true),
                gpu(
                    "NVIDIA GeForce RTX 4070 Laptop GPU",
                    Vendor::Nvidia,
                    7_948,
                    false,
                ),
            ],
        );
        let rows = vec![
            r(PK, Backend::Cpu, 0.0251, 257, 0.049, 861).row(),
            r(PK, Backend::DirectMl, 0.0251, 480, 0.09, 770)
                .vram(1_000)
                .row(),
            r(SMALL, Backend::Cpu, 0.0313, 2_740, 0.559, 562).row(),
            r(SMALL, Backend::Vulkan, 0.0313, 300, 0.06, 400)
                .est()
                .row(),
            r(BASE, Backend::Cpu, 0.0488, 819, 0.155, 275).row(),
            r(TURBO, Backend::Cpu, 0.0220, 9_000, 1.6, 1_500)
                .est()
                .row(),
            r(TURBO, Backend::Vulkan, 0.0220, 420, 0.08, 600)
                .vram(1_600)
                .est()
                .row(),
        ];
        (hw, rows)
    }

    fn amd_desktop_estimated() -> (HardwareProfile, Vec<Row>) {
        let hw = hw(
            32_000,
            20_000,
            16,
            vec![gpu("AMD Radeon RX 7800 XT", Vendor::Amd, 16_368, false)],
        );
        let rows = vec![
            r(PK, Backend::Cpu, 0.0251, 200, 0.04, 861).est().row(),
            r(PK, Backend::DirectMl, 0.0251, 150, 0.03, 800).est().row(),
            r(TURBO, Backend::Vulkan, 0.0220, 250, 0.05, 600)
                .vram(1_600)
                .est()
                .row(),
            r(SMALL, Backend::Vulkan, 0.0313, 180, 0.04, 400)
                .est()
                .row(),
        ];
        (hw, rows)
    }

    fn four_core_4gb() -> (HardwareProfile, Vec<Row>) {
        // 700 MB free: Parakeet (861 MB) doesn't fit at all.
        let hw = hw(4_000, 700, 4, vec![]);
        let rows = vec![
            r(PK, Backend::Cpu, 0.0251, 900, 0.17, 861).row(),
            r(SMALL, Backend::Cpu, 0.0313, 3_500, 0.7, 562).row(),
            r(BASE, Backend::Cpu, 0.0488, 1_200, 0.25, 275).row(),
            r(SMALL, Backend::Vulkan, 0.0313, 600, 0.1, 400).est().row(),
        ];
        (hw, rows)
    }

    fn all_fixtures() -> Vec<(HardwareProfile, Vec<Row>)> {
        vec![
            cpu_8gb_laptop(),
            rtx4070_laptop(),
            amd_desktop_estimated(),
            four_core_4gb(),
        ]
    }

    fn labels_for((hw, rows): (HardwareProfile, Vec<Row>)) -> BTreeMap<String, Vec<Label>> {
        labels(&rows, &hw, WORDS)
    }

    fn holder(l: &BTreeMap<String, Vec<Label>>, label: &Label) -> Vec<String> {
        l.iter()
            .filter(|(_, v)| v.contains(label))
            .map(|(k, _)| k.clone())
            .collect()
    }

    fn id(model: &str, backend: &str) -> String {
        format!("{model}@{backend}")
    }

    #[test]
    fn every_variant_gets_an_entry() {
        for f in all_fixtures() {
            let n = f.1.len();
            assert_eq!(labels_for(f).len(), n);
        }
    }

    #[test]
    fn exactly_one_recommended_when_anything_is_suitable() {
        for f in all_fixtures() {
            let l = labels_for(f);
            assert_eq!(holder(&l, &Label::Recommended).len(), 1, "{l:?}");
        }
    }

    #[test]
    fn rtx4070_recommends_a_fast_accurate_variant_not_the_cpu_turbo() {
        let l = labels_for(rtx4070_laptop());
        let turbo_cpu = &l[&id(TURBO, "cpu")];
        assert!(!turbo_cpu.contains(&Label::Recommended));
        assert_eq!(
            turbo_cpu[..],
            [Label::WontWorkWell {
                reason: Reason::TooSlow
            }]
        );
        // The estimated turbo is not more accurate by more than the margin, so the
        // measured Parakeet on the processor (faster) is recommended.
        assert_eq!(holder(&l, &Label::Recommended), [id(PK, "cpu")]);
    }

    #[test]
    fn a_weak_pc_still_gets_one_recommendation() {
        let l = labels_for(four_core_4gb());
        assert_eq!(
            l[&id(PK, "cpu")],
            [Label::WontWorkWell {
                reason: Reason::NotEnoughMemory { need_mb: 861 }
            }]
        );
        assert_eq!(
            l[&id(SMALL, "vulkan")],
            [Label::WontWorkWell {
                reason: Reason::NoGpu
            }]
        );
        assert_eq!(holder(&l, &Label::Recommended), [id(SMALL, "cpu")]);
    }

    #[test]
    fn ties_within_the_margin_go_to_the_faster_variant() {
        let hw = hw(16_000, 8_000, 8, vec![]);
        // 2.51% vs 2.32% at 2111 words: inside the ±0.64-pt margin.
        let rows = vec![
            r(PK, Backend::Cpu, 0.0251, 250, 0.05, 861).row(),
            r(TURBO, Backend::Cpu, 0.0232, 450, 0.09, 1_500).row(),
        ];
        let l = labels(&rows, &hw, WORDS);
        assert_eq!(holder(&l, &Label::Recommended), [id(PK, "cpu")]);
        assert_eq!(holder(&l, &Label::MostAccurate), [id(PK, "cpu")]);
        // 3.2% vs 2.32% is outside the margin: accuracy wins.
        let rows = vec![
            r(PK, Backend::Cpu, 0.0320, 250, 0.05, 861).row(),
            r(TURBO, Backend::Cpu, 0.0232, 450, 0.09, 1_500).row(),
        ];
        let l = labels(&rows, &hw, WORDS);
        assert_eq!(holder(&l, &Label::Recommended), [id(TURBO, "cpu")]);
        assert_eq!(holder(&l, &Label::MostAccurate), [id(TURBO, "cpu")]);
        assert_eq!(holder(&l, &Label::Fastest), [id(PK, "cpu")]);
    }

    #[test]
    fn measured_beats_estimated_on_ties_and_estimates_need_margin() {
        let hw = hw(16_000, 8_000, 8, vec![]);
        // Estimate a bit better on both counts: not enough, the measured one wins.
        let rows = vec![
            r(PK, Backend::Cpu, 0.0251, 300, 0.06, 861).row(),
            r(TURBO, Backend::Cpu, 0.0232, 280, 0.05, 1_500).est().row(),
        ];
        let l = labels(&rows, &hw, WORDS);
        assert_eq!(holder(&l, &Label::Recommended), [id(PK, "cpu")]);
        // Better by more than the margin and 20% faster: the estimate wins.
        let rows = vec![
            r(PK, Backend::Cpu, 0.0400, 300, 0.06, 861).row(),
            r(TURBO, Backend::Cpu, 0.0232, 200, 0.04, 1_500).est().row(),
        ];
        let l = labels(&rows, &hw, WORDS);
        assert_eq!(holder(&l, &Label::Recommended), [id(TURBO, "cpu")]);
    }

    #[test]
    fn memory_headroom_only_decides_recommended_not_whether_it_works() {
        // 3 GB needed with 4 GB free: fits, so it works here, but it is too tight to be
        // recommended when a lighter model is as good.
        let hw = hw(8_000, 4_000, 8, vec![]);
        let rows = vec![
            r(TURBO, Backend::Cpu, 0.0232, 300, 0.06, 3_000).row(),
            r(PK, Backend::Cpu, 0.0251, 350, 0.06, 861).row(),
        ];
        let l = labels(&rows, &hw, WORDS);
        assert!(
            !l[&id(TURBO, "cpu")]
                .iter()
                .any(|x| matches!(x, Label::WontWorkWell { .. })),
            "{l:?}"
        );
        assert_eq!(holder(&l, &Label::Recommended), [id(PK, "cpu")]);
        // More than is free: won't work.
        let rows = vec![r(TURBO, Backend::Cpu, 0.0232, 300, 0.06, 4_500).row()];
        let l = labels(&rows, &hw, WORDS);
        assert_eq!(
            l[&id(TURBO, "cpu")],
            [Label::WontWorkWell {
                reason: Reason::NotEnoughMemory { need_mb: 4_500 }
            }]
        );
    }

    #[test]
    fn a_measured_model_fits_by_what_it_used_not_the_install_minimum() {
        // Parakeet: catalog minimum 2 GB, measured 863 MB, 1.9 GB free.
        let hw = hw(8_000, 1_900, 8, vec![]);
        let (m, mut v, res) = r(PK, Backend::Cpu, 0.0251, 350, 0.05, 863).row();
        v.min_ram_mb = 2_048;
        let l = labels(&[(m, v, res)], &hw, WORDS);
        assert!(
            !l[&id(PK, "cpu")]
                .iter()
                .any(|x| matches!(x, Label::WontWorkWell { .. })),
            "{l:?}"
        );
    }

    #[test]
    fn a_perfect_score_does_not_shut_out_close_variants() {
        // 0 errors vs 2 errors in 396 words is within chance; the faster one wins.
        let hw = hw(16_000, 8_000, 8, vec![]);
        let rows = vec![
            r(PK, Backend::Cpu, 0.0, 687, 0.1, 861).row(),
            r(SMALL, Backend::Cpu, 2.0 / 396.0, 352, 0.05, 562).row(),
        ];
        let l = labels(&rows, &hw, 396);
        assert_eq!(holder(&l, &Label::Recommended), [id(SMALL, "cpu")]);
        assert_eq!(holder(&l, &Label::MostAccurate), [id(SMALL, "cpu")]);
        assert_eq!(holder(&l, &Label::Fastest), [id(SMALL, "cpu")]);
    }

    #[test]
    fn not_enough_gpu_memory_reason_carries_the_need() {
        let hw = hw(
            16_000,
            8_000,
            8,
            vec![gpu("NVIDIA GeForce GTX 1650", Vendor::Nvidia, 4_096, false)],
        );
        let rows = vec![
            r(TURBO, Backend::Vulkan, 0.022, 300, 0.06, 600)
                .vram(6_000)
                .est()
                .row(),
            r(PK, Backend::Cpu, 0.0251, 300, 0.06, 861).row(),
        ];
        let l = labels(&rows, &hw, WORDS);
        assert_eq!(
            l[&id(TURBO, "vulkan")],
            [Label::WontWorkWell {
                reason: Reason::NotEnoughGpuMemory { need_mb: 6_000 }
            }]
        );
    }

    #[test]
    fn gpu_variants_without_a_gpu_say_no_gpu() {
        let l = labels_for(cpu_8gb_laptop());
        for (k, v) in &l {
            if k.ends_with("@vulkan") || k.ends_with("@directml") {
                assert_eq!(
                    v[..],
                    [Label::WontWorkWell {
                        reason: Reason::NoGpu
                    }],
                    "{k}"
                );
            }
        }
    }

    #[test]
    fn a_graphics_card_run_that_timed_out_is_unstable_not_a_failed_card() {
        let hw = hw(
            16_000,
            8_000,
            8,
            vec![gpu("NVIDIA GeForce RTX 4070", Vendor::Nvidia, 8_188, false)],
        );
        let (m, v, mut res) = r(TURBO, Backend::Vulkan, 0.022, 300, 0.06, 600).row();
        // It started and transcribed, then one clip took too long.
        res.error = Some("clip c7: timed out".into());
        res.stability = Stability::Unstable {
            reason: "took too long and was stopped".into(),
        };
        let l = labels(&[(m, v, res)], &hw, WORDS);
        assert_eq!(
            l[&id(TURBO, "vulkan")],
            [Label::WontWorkWell {
                reason: Reason::Unstable {
                    detail: "took too long and was stopped".into()
                }
            }]
        );
    }

    #[test]
    fn an_estimate_needs_to_be_clearly_better_to_beat_a_measured_model() {
        // Both inside the budget; the estimate is more accurate by more than the margin
        // but not 20% faster, so the measured model stays Recommended.
        let hw = hw(16_000, 8_000, 8, vec![]);
        let rows = vec![
            r(PK, Backend::Cpu, 0.0400, 300, 0.06, 861).row(),
            r(TURBO, Backend::Cpu, 0.0232, 280, 0.05, 1_500).est().row(),
        ];
        let l = labels(&rows, &hw, WORDS);
        // The measured Parakeet is outside the margin of the best, so it isn't a candidate;
        // with no measured candidate left, the estimate is recommended.
        assert_eq!(holder(&l, &Label::Recommended), [id(TURBO, "cpu")]);
        let rows = vec![
            r(PK, Backend::Cpu, 0.0250, 300, 0.06, 861).row(),
            r(TURBO, Backend::Cpu, 0.0232, 280, 0.05, 1_500).est().row(),
        ];
        let l = labels(&rows, &hw, WORDS);
        assert_eq!(holder(&l, &Label::Recommended), [id(PK, "cpu")]);
    }

    #[test]
    fn a_gpu_that_failed_says_so() {
        let hw = hw(
            16_000,
            8_000,
            8,
            vec![gpu("AMD Radeon RX 6600", Vendor::Amd, 8_176, false)],
        );
        let (m, v, mut res) = r(PK, Backend::DirectMl, 0.0251, 300, 0.06, 861).row();
        res.metrics = None;
        res.error = Some("DirectML fell back to the processor".into());
        res.stability = Stability::Unstable {
            reason: "could not be loaded".into(),
        };
        let rows = vec![
            (m, v, res),
            r(PK, Backend::Cpu, 0.0251, 300, 0.06, 861).row(),
        ];
        let l = labels(&rows, &hw, WORDS);
        assert_eq!(
            l[&id(PK, "directml")],
            [Label::WontWorkWell {
                reason: Reason::GpuFailed {
                    detail: "DirectML fell back to the processor".into()
                }
            }]
        );
    }

    #[test]
    fn live_text_needs_streaming_and_rtf_under_0_3() {
        let hw = hw(16_000, 8_000, 8, vec![]);
        let rows = vec![
            r(MOON, Backend::Cpu, 0.05, 200, 0.2, 400).streaming().row(),
            r(PK, Backend::Cpu, 0.0251, 300, 0.06, 861).row(),
        ];
        let l = labels(&rows, &hw, WORDS);
        assert!(l[&id(MOON, "cpu")].contains(&Label::LiveText));
        assert!(!l[&id(PK, "cpu")].contains(&Label::LiveText));
        let rows = vec![r(MOON, Backend::Cpu, 0.05, 200, 0.5, 400).streaming().row()];
        let l = labels(&rows, &hw, WORDS);
        assert!(!l[&id(MOON, "cpu")].contains(&Label::LiveText));
    }

    #[test]
    fn one_variant_can_hold_several_labels() {
        let hw = hw(16_000, 8_000, 8, vec![]);
        let rows = vec![r(PK, Backend::Cpu, 0.0251, 300, 0.06, 861).row()];
        let l = labels(&rows, &hw, WORDS);
        assert_eq!(
            l[&id(PK, "cpu")],
            [Label::Recommended, Label::MostAccurate, Label::Fastest]
        );
    }

    #[test]
    fn nothing_suitable_means_no_recommended() {
        let hw = hw(16_000, 8_000, 8, vec![]);
        let mut rows = vec![
            r(PK, Backend::Cpu, 0.0251, 300, 0.06, 861).row(),
            r(SMALL, Backend::Cpu, 0.0313, 900, 0.2, 562).row(),
        ];
        for row in &mut rows {
            row.2.stability = Stability::Unstable {
                reason: "stopped working during the test".into(),
            };
        }
        let l = labels(&rows, &hw, WORDS);
        assert!(holder(&l, &Label::Recommended).is_empty());
        for v in l.values() {
            assert!(
                matches!(
                    v[..],
                    [Label::WontWorkWell {
                        reason: Reason::Unstable { .. }
                    }]
                ),
                "{v:?}"
            );
        }
    }

    #[test]
    fn a_variant_making_far_more_mistakes_than_expected_is_flagged() {
        let hw = hw(
            16_000,
            8_000,
            8,
            vec![gpu("NVIDIA GeForce RTX 4070", Vendor::Nvidia, 8_188, false)],
        );
        // Reference says 5%; on this PC it produced garbage (seen with Moonshine on
        // DirectML).
        let (m, v, mut garbage) = r(MOON, Backend::DirectMl, 0.05, 200, 0.1, 400).row();
        garbage.metrics.as_mut().unwrap().wer = 0.9;
        let (m2, v2, mut drift) = r(PK, Backend::Cpu, 0.0251, 300, 0.06, 861).row();
        drift.metrics.as_mut().unwrap().wer = 0.12;
        let rows = vec![
            (m, v, garbage),
            (m2, v2, drift),
            r(SMALL, Backend::Cpu, 0.0313, 400, 0.1, 562).row(),
        ];
        let l = labels(&rows, &hw, 396);
        for id in [id(MOON, "directml"), id(PK, "cpu")] {
            assert_eq!(
                l[&id],
                [Label::WontWorkWell {
                    reason: Reason::TooManyMistakes
                }],
                "{id}"
            );
        }
        // Within the expected range: fine.
        assert!(l[&id(SMALL, "cpu")].contains(&Label::Recommended));
    }

    #[test]
    fn a_variant_without_numbers_has_no_labels() {
        let hw = hw(16_000, 8_000, 8, vec![]);
        let (m, v, mut res) = r(SMALL, Backend::Cpu, 0.0313, 900, 0.2, 562).row();
        res.metrics = None;
        let rows = vec![
            (m, v, res),
            r(PK, Backend::Cpu, 0.0251, 300, 0.06, 861).row(),
        ];
        let l = labels(&rows, &hw, WORDS);
        assert!(l[&id(SMALL, "cpu")].is_empty());
    }
}
