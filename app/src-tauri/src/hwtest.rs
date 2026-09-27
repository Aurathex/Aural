//! Hardware Test: which models suit this PC. Pure policy lives here (tested); the
//! background runner that detects, calibrates and measures lives in `hwtest::run`.

use aural_engines::Backend;
use aural_models::{
    estimate, labels, Catalog, HardwareProfile, Label, ModelEntry, Reason, ResultsStore,
    VariantResult,
};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

/// What the Models page shows: every variant with numbers, and the labels.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Evaluation {
    pub results: Vec<VariantResult>,
    pub labels: BTreeMap<String, Vec<Label>>,
}

/// Measured results where there are any, estimates for the rest (when this PC has been
/// calibrated), then the labels over all of them.
pub fn evaluate(
    catalog: &Catalog,
    store: &ResultsStore,
    hw: &HardwareProfile,
    words: usize,
) -> Evaluation {
    let calibrations = calibrations(catalog, store);
    let mut rows = Vec::new();
    for model in catalog.models.iter().filter(|m| !m.probe) {
        for v in &model.variants {
            let result = store
                .get(&model.variant_id(v.backend))
                .cloned()
                .or_else(|| estimate(model, v, &calibrations, hw));
            if let Some(r) = result {
                rows.push((model.clone(), v.clone(), r));
            }
        }
    }
    let labels = labels(&rows, hw, words);
    Evaluation {
        results: rows.into_iter().map(|(_, _, r)| r).collect(),
        labels,
    }
}

/// The probe calibrations, plus one from any measured model with reference numbers for
/// a runtime and backend no probe covers (no probe runs ONNX models on the graphics
/// card, since Moonshine is processor-only).
fn calibrations(catalog: &Catalog, store: &ResultsStore) -> Vec<aural_models::Calibration> {
    let mut cal = store.calibrations().to_vec();
    for r in store.results().filter(|r| r.measured && r.error.is_none()) {
        let (Some(m), Some((model, v))) = (&r.metrics, catalog.variant(&r.variant)) else {
            continue;
        };
        let (Some(reference), aural_models::Stability::Stable) = (&v.reference, &r.stability)
        else {
            continue;
        };
        if !cal
            .iter()
            .any(|c| c.runtime == model.runtime && c.backend == v.backend)
        {
            cal.push(aural_models::Calibration {
                runtime: model.runtime,
                backend: v.backend,
                probe_p50_ms: m.p50_ms,
                probe_ref_p50_ms: reference.p50_ms,
            });
        }
    }
    cal
}

/// "Parakeet on the graphics card" — how a variant is named to the user.
pub fn variant_name(entry: &ModelEntry, backend: Backend) -> String {
    let place = if backend == Backend::Cpu {
        "the processor"
    } else {
        "the graphics card"
    };
    format!("{} on {place}", entry.name)
}

fn gb(mb: u64) -> String {
    let g = mb as f64 / 1024.0;
    if g >= 10.0 || (g - g.round()).abs() < 0.05 {
        format!("{} GB", g.round())
    } else {
        format!("{g:.1} GB")
    }
}

/// A reason in everyday words, completing "It …".
pub fn reason_text(r: &Reason) -> String {
    match r {
        Reason::NoGpu => "needs a separate graphics card, and this PC doesn't have one".into(),
        Reason::GpuFailed { .. } => "didn't work on this PC's graphics card".into(),
        Reason::NotEnoughMemory { need_mb } => {
            format!("needs about {} of free memory", gb(*need_mb))
        }
        Reason::NotEnoughGpuMemory { need_mb } => {
            format!("needs about {} of graphics card memory", gb(*need_mb))
        }
        Reason::TooSlow => "is too slow on this PC to keep up with speech".into(),
        Reason::TooManyMistakes => "makes too many mistakes on this PC".into(),
        Reason::Unstable { detail } => detail.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveDecision {
    Keep,
    Switch { to: String, notice: String },
}

/// After a test the user's choice stays unless it now won't work well; then Aural
/// switches to the recommended variant (or the best installed one that works) and
/// says why. With no results for the current choice (e.g. a v0.1 install before its
/// first test) nothing changes.
pub fn active_after_test(
    current: Option<&str>,
    labels: &BTreeMap<String, Vec<Label>>,
    catalog: &Catalog,
    installed: &dyn Fn(&str) -> bool,
) -> ActiveDecision {
    let Some(current) = current else {
        return ActiveDecision::Keep;
    };
    let reason = labels.get(current).and_then(|ls| {
        ls.iter().find_map(|l| match l {
            Label::WontWorkWell { reason } => Some(reason),
            _ => None,
        })
    });
    let Some(reason) = reason else {
        return ActiveDecision::Keep;
    };
    let works = |id: &&String| {
        installed(id.as_str())
            && labels[id.as_str()]
                .iter()
                .all(|l| !matches!(l, Label::WontWorkWell { .. }))
    };
    let current_model = current.rsplit_once('@').map_or(current, |(m, _)| m);
    let recommended = labels
        .iter()
        .find(|(_, ls)| ls.contains(&Label::Recommended))
        .map(|(id, _)| id)
        .filter(works);
    let same_model = labels
        .keys()
        .filter(works)
        .find(|id| id.rsplit_once('@').is_some_and(|(m, _)| m == current_model));
    let Some(to) = recommended
        .or(same_model)
        .or_else(|| labels.keys().find(works))
    else {
        return ActiveDecision::Keep;
    };
    let name = |id: &str| {
        catalog
            .variant(id)
            .map_or_else(|| id.to_owned(), |(m, v)| variant_name(m, v.backend))
    };
    ActiveDecision::Switch {
        to: to.clone(),
        notice: format!(
            "{} won't work well on this PC: it {}. Aural switched to {}.",
            name(current),
            reason_text(reason),
            name(to)
        ),
    }
}

/// Backends to measure a downloaded model on: always the processor, plus its graphics
/// variants when the PC has a separate graphics card. Measuring is what proves a
/// backend works; a failure shows up as "didn't work on this PC's graphics card".
pub fn backends_to_measure(entry: &ModelEntry, hw: &HardwareProfile) -> Vec<Backend> {
    let gpu = aural_platform::gpu::primary_discrete(&hw.gpus).is_some();
    entry
        .variants
        .iter()
        .map(|v| v.backend)
        .filter(|b| *b == Backend::Cpu || gpu)
        .collect()
}

/// Variants waiting to be measured, one at a time, oldest first, no duplicates.
#[derive(Debug, Default)]
pub struct MeasureQueue {
    items: VecDeque<String>,
}

impl MeasureQueue {
    pub fn push(&mut self, variant: String) {
        if !self.items.contains(&variant) {
            self.items.push_back(variant);
        }
    }

    pub fn pop(&mut self) -> Option<String> {
        self.items.pop_front()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// A finished download queues all its variants this PC can run.
    pub fn on_download_complete(&mut self, entry: &ModelEntry, hw: &HardwareProfile) {
        for b in backends_to_measure(entry, hw) {
            self.push(entry.variant_id(b));
        }
    }
}

// ---------------------------------------------------------------------------------
// Runtime: one background thread runs jobs in order (a full test, or measuring one
// downloaded variant), so two benchmarks never compete with each other.

use crate::state::{lock, App};
use aural_eval::clips::Clip;
use aural_models::Calibration;
use aural_stt_protocol::bench::{benchmark_variant, BenchTarget, Stability};
use aural_stt_protocol::client::WorkerSpec;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;
use tauri::{Emitter, Manager};

/// Passes over the clips when measuring a variant, and when calibrating with a probe.
const MEASURE_PASSES: usize = 3;
const CALIBRATE_PASSES: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum HwStep {
    Detecting,
    DownloadingProbes,
    Calibrating { backend: Backend },
    Measuring { variant: String },
    Estimating,
    Done,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct HwTestStatus {
    pub running: bool,
    pub step: Option<HwStep>,
    pub done: usize,
    pub total: usize,
    /// A test has been run on this PC at least once.
    pub tested: bool,
    /// The saved results are from different hardware (new graphics card or driver).
    pub stale: bool,
}

enum Job {
    FullTest { allow_probe_download: bool },
    Measure(String),
}

pub struct HwTest {
    pub results: Mutex<ResultsStore>,
    status: Mutex<HwTestStatus>,
    jobs: Mutex<VecDeque<Job>>,
    wake: Condvar,
    cancel: Arc<AtomicBool>,
    clips: OnceLock<Result<Vec<Clip>, String>>,
}

impl HwTest {
    pub fn new(results: ResultsStore) -> HwTest {
        HwTest {
            results: Mutex::new(results),
            status: Mutex::new(HwTestStatus::default()),
            jobs: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            clips: OnceLock::new(),
        }
    }

    pub fn status(&self, hw: &HardwareProfile) -> HwTestStatus {
        let results = lock(&self.results);
        HwTestStatus {
            tested: results.calibrations().len() + results.results().count() > 0,
            stale: results.is_stale(hw),
            ..lock(&self.status).clone()
        }
    }

    fn push(&self, job: Job) {
        lock(&self.jobs).push_back(job);
        self.wake.notify_one();
    }
}

/// Starts the job thread (once, at startup).
pub fn spawn_runner(app: &Arc<App>) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("aural-hwtest".into())
        .spawn(move || loop {
            let job = {
                let mut jobs = lock(&app.hwtest.jobs);
                loop {
                    if let Some(j) = jobs.pop_front() {
                        break j;
                    }
                    jobs = app
                        .hwtest
                        .wake
                        .wait(jobs)
                        .unwrap_or_else(|p| p.into_inner());
                }
            };
            app.hwtest.cancel.store(false, Ordering::SeqCst);
            match job {
                Job::FullTest {
                    allow_probe_download,
                } => full_test(&app, allow_probe_download),
                Job::Measure(variant) => {
                    set_step(
                        &app,
                        Some(HwStep::Measuring {
                            variant: variant.clone(),
                        }),
                        0,
                        1,
                    );
                    measure(&app, &variant);
                    set_step(&app, None, 0, 0);
                }
            }
            apply_active_policy(&app);
            app.broadcast();
        });
}

pub fn start(app: &Arc<App>, allow_probe_download: bool) {
    if lock(&app.hwtest.status).running {
        return;
    }
    lock(&app.hwtest.status).running = true;
    app.hwtest.push(Job::FullTest {
        allow_probe_download,
    });
}

pub fn cancel(app: &Arc<App>) {
    let mut jobs = lock(&app.hwtest.jobs);
    // A test that was queued but never started must not look like it is running.
    if jobs.iter().any(|j| matches!(j, Job::FullTest { .. })) {
        lock(&app.hwtest.status).running = false;
    }
    jobs.clear();
    drop(jobs);
    app.hwtest.cancel.store(true, Ordering::SeqCst);
}

/// After a download: measure the new model on what this PC can run.
pub fn measure_download(app: &Arc<App>, entry: &ModelEntry) {
    if entry.probe {
        return;
    }
    let mut q = MeasureQueue::default();
    q.on_download_complete(entry, &app.hw);
    while let Some(v) = q.pop() {
        app.hwtest.push(Job::Measure(v));
    }
}

/// Current results and labels for the Models page.
pub fn evaluation(app: &App) -> Evaluation {
    let words = clips(app).map_or(0, clip_words);
    evaluate(&app.catalog, &lock(&app.hwtest.results), &app.hw, words)
}

fn clip_words(clips: &[Clip]) -> usize {
    clips
        .iter()
        .map(|c| aural_eval::wer::normalize(&c.reference).len())
        .sum()
}

fn clips(app: &App) -> Result<&[Clip], String> {
    app.hwtest
        .clips
        .get_or_init(|| {
            let dir = app
                .handle
                .path()
                .resource_dir()
                .map_err(|e| e.to_string())?
                .join("eval");
            aural_eval::clips::builtin(&dir).map_err(|e| format!("{e:#}"))
        })
        .as_deref()
        .map_err(Clone::clone)
}

fn set_step(app: &App, step: Option<HwStep>, done: usize, total: usize) {
    {
        let mut s = lock(&app.hwtest.status);
        s.step = step;
        s.done = done;
        s.total = total;
    }
    let _ = app
        .handle
        .emit_to("main", "hwtest-progress", app.hwtest.status(&app.hw));
}

fn cancelled(app: &App) -> bool {
    app.hwtest.cancel.load(Ordering::SeqCst)
}

/// Dictation comes first: wait while the user is speaking or Aural is transcribing.
fn wait_for_idle(app: &App) {
    while !app.pill_hidden.load(Ordering::SeqCst) && !cancelled(app) {
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn run_bench(app: &App, entry: &ModelEntry, backend: Backend, passes: usize) -> VariantResult {
    let variant = entry.variant_id(backend);
    let clips = match clips(app) {
        Ok(c) => c,
        Err(e) => {
            return VariantResult {
                variant,
                metrics: None,
                load_ms: 0,
                ram_mb: 0,
                vram_mb: None,
                spread: 0.0,
                passes: 0,
                stability: Stability::Unstable {
                    reason: "could not be tested: the built-in test recordings are missing".into(),
                },
                measured: true,
                error: Some(e),
            }
        }
    };
    wait_for_idle(app);
    benchmark_variant(
        WorkerSpec {
            exe: crate::engine::worker_exe(entry.engine),
            args: vec![],
        },
        &BenchTarget {
            variant,
            model_path: crate::engine::model_path(entry, &app.store),
            engine: entry.engine,
            backend,
            threads: crate::engine::threads(),
        },
        clips,
        passes,
        &app.hwtest.cancel,
    )
}

fn measure(app: &App, variant: &str) {
    let Some((entry, v)) = app.catalog.variant(variant) else {
        return;
    };
    if !app.store.is_installed(entry) {
        return;
    }
    let r = run_bench(app, entry, v.backend, MEASURE_PASSES);
    if r.error.as_deref() == Some("cancelled") {
        return;
    }
    let mut results = lock(&app.hwtest.results);
    results.adopt(&app.hw);
    results.put(r);
    let _ = results.save();
}

fn full_test(app: &Arc<App>, allow_probe_download: bool) {
    set_step(app, Some(HwStep::Detecting), 0, 0);
    {
        let mut results = lock(&app.hwtest.results);
        results.reset_for(&app.hw);
        let _ = results.save();
    }
    app.broadcast();

    // Probes: two small models, one per runtime, to learn how fast this PC is.
    let probes: Vec<ModelEntry> = app
        .catalog
        .models
        .iter()
        .filter(|m| m.probe)
        .cloned()
        .collect();
    if allow_probe_download {
        let missing: Vec<&ModelEntry> = probes
            .iter()
            .filter(|p| !app.store.is_installed(p))
            .collect();
        for (i, p) in missing.iter().enumerate() {
            if cancelled(app) {
                break;
            }
            set_step(app, Some(HwStep::DownloadingProbes), i, missing.len());
            if let Err(e) = aural_models::download(p, &app.store, &app.hwtest.cancel, &mut |_| {}) {
                if !matches!(e, aural_models::DownloadError::Cancelled) {
                    app.set_notice(format!(
                        "The hardware test couldn't download its small test models ({e}). Results will appear once you download a model."
                    ));
                }
            }
        }
    }

    let mut calibrations = Vec::new();
    for p in probes.iter().filter(|p| app.store.is_installed(p)) {
        for b in backends_to_measure(p, &app.hw) {
            if cancelled(app) {
                break;
            }
            set_step(
                app,
                Some(HwStep::Calibrating { backend: b }),
                calibrations.len(),
                0,
            );
            let r = run_bench(app, p, b, CALIBRATE_PASSES);
            let reference = p.variant(b).and_then(|v| v.reference.as_ref());
            if let (Some(m), Some(re), None) = (r.metrics, reference, &r.error) {
                calibrations.push(Calibration {
                    runtime: p.runtime,
                    backend: b,
                    probe_p50_ms: m.p50_ms,
                    probe_ref_p50_ms: re.p50_ms,
                });
            }
        }
    }
    lock(&app.hwtest.results).set_calibrations(calibrations);

    // Models already downloaded get measured for real.
    let installed: Vec<String> = app
        .catalog
        .models
        .iter()
        .filter(|m| !m.probe && app.store.is_installed(m))
        .flat_map(|m| {
            backends_to_measure(m, &app.hw)
                .into_iter()
                .map(|b| m.variant_id(b))
                .collect::<Vec<_>>()
        })
        .collect();
    for (i, v) in installed.iter().enumerate() {
        if cancelled(app) {
            break;
        }
        set_step(
            app,
            Some(HwStep::Measuring { variant: v.clone() }),
            i,
            installed.len(),
        );
        measure(app, v);
    }

    set_step(app, Some(HwStep::Estimating), 0, 0);
    let _ = lock(&app.hwtest.results).save();
    lock(&app.hwtest.status).running = false;
    set_step(app, Some(HwStep::Done), 0, 0);
}

/// Keeps the user's model unless it now won't work well here; then switches and says so.
fn apply_active_policy(app: &Arc<App>) {
    let e = evaluation(app);
    let current = app.settings().stt.active_variant;
    let installed = |id: &str| {
        app.catalog
            .variant(id)
            .is_some_and(|(m, _)| app.store.is_installed(m))
    };
    if let ActiveDecision::Switch { to, notice } =
        active_after_test(current.as_deref(), &e.labels, &app.catalog, &installed)
    {
        let mut s = app.settings();
        s.stt.active_model =
            aural_models::catalog::split_variant_id(&to).map(|(m, _)| m.to_owned());
        s.stt.active_variant = Some(to);
        if app.save_settings(s).is_ok() {
            app.set_notice(notice);
            app.reload_engine();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aural_platform::gpu::{GpuInfo, Vendor};

    const JARGON: &[&str] = &[
        "WER", "RTF", "VRAM", "DirectML", "Vulkan", "ONNX", "ggml", "int8", "backend",
    ];

    fn laptop(with_gpu: bool) -> HardwareProfile {
        HardwareProfile {
            cpu_name: "cpu".into(),
            total_ram_mb: 16_000,
            free_ram_mb: 8_000,
            logical_cores: 8,
            physical_cores: 4,
            avx2: true,
            gpus: if with_gpu {
                vec![GpuInfo {
                    name: "NVIDIA GeForce RTX 4070 Laptop GPU".into(),
                    vendor: Vendor::Nvidia,
                    vram_mb: 7_948,
                    integrated: false,
                    driver: "1".into(),
                    luid: 1,
                }]
            } else {
                vec![]
            },
            ..HardwareProfile::default()
        }
    }

    fn catalog_with_directml() -> Catalog {
        let mut c = Catalog::builtin();
        let pk = c
            .models
            .iter_mut()
            .find(|m| m.id == "parakeet-tdt-0.6b-v2-int8")
            .unwrap();
        if pk.variant(Backend::DirectMl).is_none() {
            let mut v = pk.variants[0].clone();
            v.backend = Backend::DirectMl;
            pk.variants.push(v);
        }
        c
    }

    const PK_CPU: &str = "parakeet-tdt-0.6b-v2-int8@cpu";
    const PK_DML: &str = "parakeet-tdt-0.6b-v2-int8@directml";
    const SMALL_CPU: &str = "whisper-small.en-q8@cpu";

    fn www(reason: Reason) -> Vec<Label> {
        vec![Label::WontWorkWell { reason }]
    }

    #[test]
    fn v01_install_keeps_working_before_the_hardware_test() {
        let d = active_after_test(Some(PK_CPU), &BTreeMap::new(), &Catalog::builtin(), &|_| {
            true
        });
        assert_eq!(d, ActiveDecision::Keep);
    }

    #[test]
    fn gpu_failure_falls_back_to_recommended_cpu_with_notice() {
        let labels = BTreeMap::from([
            (
                PK_DML.to_string(),
                www(Reason::GpuFailed {
                    detail: "DirectML fell back to CPU".into(),
                }),
            ),
            (PK_CPU.to_string(), vec![Label::Recommended]),
        ]);
        let ActiveDecision::Switch { to, notice } =
            active_after_test(Some(PK_DML), &labels, &catalog_with_directml(), &|_| true)
        else {
            panic!("expected a switch")
        };
        assert_eq!(to, PK_CPU);
        assert!(notice.contains("graphics card"), "{notice}");
        assert!(notice.contains("processor"), "{notice}");
        for word in JARGON {
            assert!(!notice.contains(word), "{word} in: {notice}");
        }
    }

    #[test]
    fn falls_back_to_a_working_installed_variant_when_recommended_is_not_installed() {
        let labels = BTreeMap::from([
            (PK_DML.to_string(), www(Reason::TooSlow)),
            (PK_CPU.to_string(), vec![]),
            (SMALL_CPU.to_string(), vec![Label::Recommended]),
        ]);
        let installed = |id: &str| id.starts_with("parakeet");
        let d = active_after_test(Some(PK_DML), &labels, &catalog_with_directml(), &installed);
        assert!(
            matches!(d, ActiveDecision::Switch { ref to, .. } if to == PK_CPU),
            "{d:?}"
        );
    }

    #[test]
    fn active_choice_kept_unless_it_wont_work_well() {
        let c = catalog_with_directml();
        let labels = BTreeMap::from([
            (PK_CPU.to_string(), vec![Label::Fastest]),
            (SMALL_CPU.to_string(), vec![Label::Recommended]),
        ]);
        assert_eq!(
            active_after_test(Some(PK_CPU), &labels, &c, &|_| true),
            ActiveDecision::Keep
        );
        let labels = BTreeMap::from([
            (PK_CPU.to_string(), www(Reason::TooSlow)),
            (SMALL_CPU.to_string(), vec![Label::Recommended]),
        ]);
        assert!(matches!(
            active_after_test(Some(PK_CPU), &labels, &c, &|_| true),
            ActiveDecision::Switch { ref to, .. } if to == SMALL_CPU
        ));
        // Nothing installed that works: keep what there is rather than nothing.
        assert_eq!(
            active_after_test(Some(PK_CPU), &labels, &c, &|id: &str| id == PK_CPU),
            ActiveDecision::Keep
        );
    }

    #[test]
    fn declined_probe_shows_detection_only() {
        let dir = tempfile::tempdir().unwrap();
        let store = ResultsStore::load(&dir.path().join("hardware.json")).unwrap();
        let e = evaluate(&Catalog::builtin(), &store, &laptop(true), 396);
        assert!(e.results.is_empty());
        assert!(e.labels.is_empty());
    }

    #[test]
    fn measured_results_are_labelled() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ResultsStore::load(&dir.path().join("hardware.json")).unwrap();
        store.put(VariantResult {
            variant: PK_CPU.into(),
            metrics: Some(aural_eval::metrics::RunMetrics {
                wer: 0.005,
                words: 396,
                p50_ms: 350,
                p95_ms: 548,
                rtf: 0.05,
            }),
            load_ms: 1_858,
            ram_mb: 772,
            vram_mb: None,
            spread: 1.6,
            passes: 3,
            stability: aural_models::Stability::Stable,
            measured: true,
            error: None,
        });
        let e = evaluate(&Catalog::builtin(), &store, &laptop(false), 396);
        assert!(e.results.iter().any(|r| r.variant == PK_CPU && r.measured));
        // It also calibrates the processor for the other ONNX models, which get estimates.
        assert!(e
            .results
            .iter()
            .any(|r| r.variant == "moonshine-base-int8@cpu" && !r.measured));
        assert_eq!(
            e.labels[PK_CPU],
            [Label::Recommended, Label::MostAccurate, Label::Fastest]
        );
    }

    #[test]
    fn a_measured_model_calibrates_its_backend_for_estimates() {
        // No probe runs on the graphics card for ONNX models (Moonshine is CPU-only), so a
        // measured Parakeet there is what lets Aural estimate the other ONNX models there.
        let mut c = catalog_with_directml();
        let reference = |p50_ms: u64| aural_models::catalog::Reference {
            wer: 0.02,
            p50_ms,
            rtf: 0.05,
            load_ms: 2_000,
            ram_mb: 800,
            vram_mb: 1_000,
        };
        for m in c.models.iter_mut().filter(|m| m.family == "parakeet") {
            for v in m.variants.iter_mut() {
                v.reference = Some(reference(if m.id.contains("v3") { 400 } else { 350 }));
            }
            if m.variant(Backend::DirectMl).is_none() {
                let mut v = m.variants[0].clone();
                v.backend = Backend::DirectMl;
                m.variants.push(v);
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut store = ResultsStore::load(&dir.path().join("hardware.json")).unwrap();
        store.put(VariantResult {
            variant: PK_DML.into(),
            metrics: Some(aural_eval::metrics::RunMetrics {
                wer: 0.005,
                words: 396,
                p50_ms: 700,
                p95_ms: 800,
                rtf: 0.09,
            }),
            load_ms: 3_000,
            ram_mb: 770,
            vram_mb: Some(1_000),
            spread: 1.2,
            passes: 3,
            stability: aural_models::Stability::Stable,
            measured: true,
            error: None,
        });
        let e = evaluate(&c, &store, &laptop(true), 396);
        let v3 = e
            .results
            .iter()
            .find(|r| r.variant == "parakeet-tdt-0.6b-v3-int8@directml")
            .expect("parakeet v3 on the graphics card is estimated");
        assert!(!v3.measured);
        // 400 ms reference × (700 measured / 350 reference) for Parakeet v2 there.
        assert_eq!(v3.metrics.unwrap().p50_ms, 800);
    }

    #[test]
    fn download_triggers_measurement() {
        let c = catalog_with_directml();
        let pk = c.get("parakeet-tdt-0.6b-v2-int8").unwrap();
        let mut q = MeasureQueue::default();
        q.on_download_complete(pk, &laptop(true));
        assert_eq!(q.pop().as_deref(), Some(PK_CPU));
        assert_eq!(q.pop().as_deref(), Some(PK_DML));
        assert!(q.is_empty());
        // Without a separate graphics card only the processor is measured.
        q.on_download_complete(pk, &laptop(false));
        q.on_download_complete(pk, &laptop(false));
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn reasons_are_plain_words() {
        let reasons = [
            Reason::NoGpu,
            Reason::GpuFailed {
                detail: "ORT DirectML EP init failed".into(),
            },
            Reason::NotEnoughMemory { need_mb: 6_144 },
            Reason::NotEnoughGpuMemory { need_mb: 6_144 },
            Reason::TooSlow,
            Reason::TooManyMistakes,
            Reason::Unstable {
                detail: "stopped working during the test".into(),
            },
        ];
        for r in &reasons {
            let t = reason_text(r);
            assert!(!t.is_empty());
            for word in JARGON {
                assert!(!t.contains(word), "{word} in: {t}");
            }
        }
        assert!(reason_text(&Reason::NotEnoughGpuMemory { need_mb: 6_144 }).contains("6 GB"));
    }

    #[test]
    fn variants_are_named_by_where_they_run() {
        let c = Catalog::builtin();
        let pk = c.get("parakeet-tdt-0.6b-v2-int8").unwrap();
        assert!(variant_name(pk, Backend::Cpu).ends_with("on the processor"));
        assert!(variant_name(pk, Backend::Vulkan).ends_with("on the graphics card"));
    }
}
