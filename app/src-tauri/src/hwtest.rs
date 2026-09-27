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
    let stale = store.is_stale(hw);
    let mut rows = Vec::new();
    for model in catalog.models.iter().filter(|m| !m.probe) {
        for v in &model.variants {
            let result = store
                .get(&model.variant_id(v.backend))
                .cloned()
                // Measured on different hardware (new card, driver): only a guess now.
                .map(|mut r| {
                    r.measured &= !stale;
                    r
                })
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

/// Free memory for the labels: what is free now plus what the loaded dictation model
/// holds (it would be released to run another one, and it obviously fits itself).
pub fn free_for_labels(free_mb: u64, loaded_model_mb: Option<u64>) -> u64 {
    free_mb + loaded_model_mb.unwrap_or(0)
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
    // Free memory changes all the time and the model is running: not a reason to switch.
    if matches!(reason, Reason::NotEnoughMemory { .. }) {
        return ActiveDecision::Keep;
    }
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

/// A model that couldn't start on the graphics card switches to the processor: the same
/// model there if it can, else the recommended installed variant on the processor, else
/// any installed one. Returns the variant and the notice for the user. `None` for a
/// processor failure (nothing to fall back to) or when nothing else is installed.
pub fn fallback_after_load_failure(
    failed: &str,
    labels: &BTreeMap<String, Vec<Label>>,
    catalog: &Catalog,
    installed: &dyn Fn(&str) -> bool,
) -> Option<(String, String)> {
    let (model, backend) = aural_models::catalog::split_variant_id(failed)?;
    if backend == Backend::Cpu {
        return None;
    }
    let cpu_installed = |id: &str| {
        installed(id)
            && catalog
                .variant(id)
                .is_some_and(|(_, v)| v.backend == Backend::Cpu)
    };
    // Not one already known to work badly here (e.g. far too slow on the processor).
    let cpu_ok = |id: &str| {
        cpu_installed(id)
            && labels
                .get(id)
                .is_none_or(|ls| !ls.iter().any(|l| matches!(l, Label::WontWorkWell { .. })))
    };
    let same = catalog
        .get(model)
        .map(|m| m.variant_id(Backend::Cpu))
        .filter(|id| cpu_ok(id));
    let recommended = labels
        .iter()
        .find(|(id, ls)| ls.contains(&Label::Recommended) && cpu_ok(id))
        .map(|(id, _)| id.clone());
    let any = || {
        catalog
            .models
            .iter()
            .filter(|m| !m.probe)
            .map(|m| m.variant_id(Backend::Cpu))
            .find(|id| cpu_ok(id))
    };
    // Last resort: the same model on the processor even if slow, rather than nothing.
    let last_resort = || {
        catalog
            .get(model)
            .map(|m| m.variant_id(Backend::Cpu))
            .filter(|id| cpu_installed(id))
    };
    let to = same.or(recommended).or_else(any).or_else(last_resort)?;
    Some((
        to,
        "Your graphics card couldn't start this model, so Aural is using your processor instead. You can keep dictating."
            .into(),
    ))
}

/// Which variant of a freshly downloaded model to use: the one labelled Recommended,
/// else the fastest one that isn't marked "won't work well here", else the processor.
pub fn variant_to_activate(
    entry: &ModelEntry,
    labels: &BTreeMap<String, Vec<Label>>,
    results: &[VariantResult],
) -> String {
    let ids: Vec<String> = entry
        .variants
        .iter()
        .map(|v| entry.variant_id(v.backend))
        .collect();
    let bad = |id: &str| {
        labels
            .get(id)
            .is_some_and(|ls| ls.iter().any(|l| matches!(l, Label::WontWorkWell { .. })))
    };
    if let Some(r) = ids.iter().find(|id| {
        labels
            .get(*id)
            .is_some_and(|ls| ls.contains(&Label::Recommended))
    }) {
        return r.clone();
    }
    ids.iter()
        .filter(|id| !bad(id))
        .filter_map(|id| {
            let p50 = results
                .iter()
                .find(|r| r.variant == **id)?
                .metrics
                .as_ref()?
                .p50_ms;
            Some((p50, id))
        })
        .min()
        .map(|(_, id)| id.clone())
        .unwrap_or_else(|| entry.variant_id(Backend::Cpu))
}

/// Backends to measure a downloaded model on: always the processor, plus its graphics
/// variants when the PC has a separate graphics card. Measuring is what proves a
/// backend works; the graphics card goes first. A failure shows up as "didn't work on this PC's graphics card".
pub fn backends_to_measure(entry: &ModelEntry, hw: &HardwareProfile) -> Vec<Backend> {
    let gpu = aural_platform::gpu::primary_discrete(&hw.gpus).is_some();
    entry
        .variants
        .iter()
        .map(|v| v.backend)
        .filter(|b| *b == Backend::Cpu || gpu)
        // Graphics card first: quick to measure, and it decides whether the model is
        // usable there before a possibly slow processor run.
        .rev()
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
    /// The small test models are already downloaded (a re-check needs no download).
    pub test_models_installed: bool,
}

enum Job {
    FullTest { allow_probe_download: bool },
    Measure(String),
}

/// Removes queued full checks (cancel), keeping measurements of new downloads. Returns
/// whether any check was removed.
fn drop_tests(jobs: &mut VecDeque<Job>) -> bool {
    let before = jobs.len();
    jobs.retain(|j| matches!(j, Job::Measure(_)));
    jobs.len() != before
}

/// What a full check collects; it replaces the saved results only once the check has
/// finished, so a cancelled check keeps the previous ones.
#[derive(Debug, Default)]
struct TestRun {
    calibrations: Vec<Calibration>,
    results: Vec<VariantResult>,
}

impl TestRun {
    fn commit_if_complete(&self, store: &mut ResultsStore, hw: &HardwareProfile, complete: bool) {
        if !complete {
            return;
        }
        store.reset_for(hw);
        store.mark_checked();
        store.set_calibrations(self.calibrations.clone());
        for r in &self.results {
            store.put(r.clone());
        }
    }
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
            tested: results.checked(),
            stale: results.is_stale(hw),
            ..lock(&self.status).clone()
        }
    }

    fn push(&self, job: Job) {
        lock(&self.jobs).push_back(job);
        self.wake.notify_one();
    }
}

/// The check's status, including whether its test models are already on this PC.
pub fn status(app: &App) -> HwTestStatus {
    HwTestStatus {
        test_models_installed: app
            .catalog
            .models
            .iter()
            .filter(|m| m.probe)
            .all(|m| app.store.is_installed(m)),
        ..app.hwtest.status(&app.hw)
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
                        // Cleared under the jobs lock: a cancel that drops queued jobs
                        // can't land between taking this job and clearing the flag.
                        app.hwtest.cancel.store(false, Ordering::SeqCst);
                        break j;
                    }
                    jobs = app
                        .hwtest
                        .wake
                        .wait(jobs)
                        .unwrap_or_else(|p| p.into_inner());
                }
            };
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
                    if let Some(r) = measure(&app, &variant) {
                        record(&app, r);
                    }
                    set_step(&app, None, 0, 0);
                }
            }
            apply_active_policy(&app);
            app.broadcast();
        });
}

pub fn start(app: &Arc<App>, allow_probe_download: bool) {
    {
        // Check and set together, so two quick clicks queue one check.
        let mut status = lock(&app.hwtest.status);
        if status.running {
            return;
        }
        status.running = true;
    }
    app.hwtest.push(Job::FullTest {
        allow_probe_download,
    });
}

pub fn cancel(app: &Arc<App>) {
    // Measurements of new downloads stay queued; a check that was queued but never
    // started must not look like it is running.
    let dropped = drop_tests(&mut lock(&app.hwtest.jobs));
    if dropped {
        lock(&app.hwtest.status).running = false;
    }
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
    // Memory limits use what is free now, not what was free when Aural started.
    let mut hw = app.hw.clone();
    let free = aural_models::recommend::free_ram_mb();
    let results = lock(&app.hwtest.results);
    if free > 0 {
        let loaded = app
            .engine
            .is_ready()
            .then(|| app.settings().stt.active_variant)
            .flatten()
            .and_then(|v| results.get(&v).map(|r| r.ram_mb));
        hw.free_ram_mb = free_for_labels(free, loaded);
    }
    evaluate(&app.catalog, &results, &hw, words)
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
    let _ = app.handle.emit_to("main", "hwtest-progress", status(app));
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

/// Measures one installed variant; `None` if it isn't installed or was cancelled.
fn measure(app: &App, variant: &str) -> Option<VariantResult> {
    let (entry, v) = app.catalog.variant(variant)?;
    if !app.store.is_installed(entry) {
        return None;
    }
    let r = run_bench(app, entry, v.backend, MEASURE_PASSES);
    (r.error.as_deref() != Some("cancelled")).then_some(r)
}

/// Stores a result measured outside a full check (after a download, or a failed load).
fn record(app: &App, r: VariantResult) {
    let mut results = lock(&app.hwtest.results);
    results.adopt(&app.hw);
    results.put(r);
    let _ = results.save();
}

fn full_test(app: &Arc<App>, allow_probe_download: bool) {
    set_step(app, Some(HwStep::Detecting), 0, 0);
    let mut run = TestRun::default();

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
                    app.set_notice(
                        "Aural couldn't download its small test models, so it can only compare models you download. Check your internet connection and try again.",
                    );
                }
            }
        }
    }

    for p in probes.iter().filter(|p| app.store.is_installed(p)) {
        for b in backends_to_measure(p, &app.hw) {
            if cancelled(app) {
                break;
            }
            set_step(app, Some(HwStep::Calibrating { backend: b }), 0, 0);
            let r = run_bench(app, p, b, CALIBRATE_PASSES);
            let reference = p.variant(b).and_then(|v| v.reference.as_ref());
            if let (Some(m), Some(re), None) = (r.metrics, reference, &r.error) {
                run.calibrations.push(Calibration {
                    runtime: p.runtime,
                    backend: b,
                    probe_p50_ms: m.p50_ms,
                    probe_ref_p50_ms: re.p50_ms,
                });
            }
        }
    }

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
        if let Some(r) = measure(app, v) {
            run.results.push(r);
        }
    }

    set_step(app, Some(HwStep::Estimating), 0, 0);
    {
        // A cancelled check keeps the previous results.
        let mut results = lock(&app.hwtest.results);
        run.commit_if_complete(&mut results, &app.hw, !cancelled(app));
        let _ = results.save();
    }
    lock(&app.hwtest.status).running = false;
    set_step(app, Some(HwStep::Done), 0, 0);
}

/// A model that failed to load on the graphics card: remember that it didn't work there,
/// and move dictation to the processor so the user can keep going.
pub fn after_load_failure(app: &Arc<App>, variant: &str, message: &str) {
    record(
        app,
        VariantResult {
            variant: variant.to_owned(),
            metrics: None,
            load_ms: 0,
            ram_mb: 0,
            vram_mb: None,
            spread: 0.0,
            passes: 0,
            stability: Stability::Unstable {
                reason: "could not be started".into(),
            },
            measured: true,
            error: Some(message.to_owned()),
        },
    );
    let e = evaluation(app);
    let installed = |id: &str| {
        app.catalog
            .variant(id)
            .is_some_and(|(m, _)| app.store.is_installed(m))
    };
    if let Some((to, notice)) =
        fallback_after_load_failure(variant, &e.labels, &app.catalog, &installed)
    {
        if app.update_settings(|s| set_active(s, &to)).is_ok() {
            app.set_notice(notice);
            app.reload_engine();
        }
    }
    app.broadcast();
}
fn set_active(s: &mut aural_core::settings::Settings, variant: &str) {
    s.stt.active_model =
        aural_models::catalog::split_variant_id(variant).map(|(m, _)| m.to_owned());
    s.stt.active_variant = Some(variant.to_owned());
}

/// Keeps the user's model unless it now won't work well here; then switches and says so.
fn apply_active_policy(app: &Arc<App>) {
    // Results from other hardware aren't a reason to switch; the user is asked to re-check.
    if lock(&app.hwtest.results).is_stale(&app.hw) {
        return;
    }
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
        // Don't swap the model out from under a dictation in progress.
        wait_for_idle(app);
        if app.update_settings(|s| set_active(s, &to)).is_ok() {
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
        // Measured beats the (equally accurate, slightly faster) Parakeet v3 estimate.
        assert!(
            e.labels[PK_CPU].contains(&Label::Recommended),
            "{:?}",
            e.labels
        );
    }

    #[test]
    fn a_measured_model_calibrates_its_backend_for_estimates() {
        // No probe runs on the graphics card for ONNX models (Moonshine is CPU-only), so a
        // measured Parakeet there is what lets Aural estimate the other ONNX models there.
        let mut c = catalog_with_directml();
        let reference = |p50_ms: u64| aural_models::catalog::Reference {
            wer: 0.02,
            clips_wer: None,
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
    fn a_graphics_card_that_fails_to_load_falls_back_to_the_processor() {
        let c = catalog_with_directml();
        let d = fallback_after_load_failure(PK_DML, &BTreeMap::new(), &c, &|_| true).unwrap();
        assert_eq!(d.0, PK_CPU);
        assert!(d.1.contains("graphics card"), "{}", d.1);
        assert!(d.1.contains("processor"), "{}", d.1);
        assert!(d.1.contains("keep dictating"), "{}", d.1);
        for word in JARGON {
            assert!(!d.1.contains(word), "{word} in: {}", d.1);
        }
        // The processor failing has no fallback here: that error is shown as it is.
        assert!(fallback_after_load_failure(PK_CPU, &BTreeMap::new(), &c, &|_| true).is_none());
        // Prefer the recommended processor variant when this model has none.
        let labels = BTreeMap::from([(SMALL_CPU.to_string(), vec![Label::Recommended])]);
        let turbo_vk = "whisper-large-v3-turbo-q5@vulkan";
        let only_small = |id: &str| id.starts_with("whisper-small");
        assert_eq!(
            fallback_after_load_failure(turbo_vk, &labels, &c, &only_small).map(|d| d.0),
            Some(SMALL_CPU.to_string())
        );
    }

    #[test]
    fn a_failed_graphics_card_never_falls_back_to_a_too_slow_processor_variant() {
        let c = catalog_with_directml();
        let turbo_vk = "whisper-large-v3-turbo-q5@vulkan";
        let turbo_cpu = "whisper-large-v3-turbo-q5@cpu";
        let labels = BTreeMap::from([
            (turbo_cpu.to_string(), www(Reason::TooSlow)),
            (PK_CPU.to_string(), vec![]),
        ]);
        let installed =
            |id: &str| id.starts_with("whisper-large") || id.starts_with("parakeet-tdt-0.6b-v2");
        assert_eq!(
            fallback_after_load_failure(turbo_vk, &labels, &c, &installed).map(|d| d.0),
            Some(PK_CPU.to_string())
        );
        // Nothing else installed: the slow processor variant is still better than no
        // dictation at all.
        let only_turbo = |id: &str| id.starts_with("whisper-large");
        assert_eq!(
            fallback_after_load_failure(turbo_vk, &labels, &c, &only_turbo).map(|d| d.0),
            Some(turbo_cpu.to_string())
        );
    }

    #[test]
    fn a_loaded_model_is_not_switched_away_just_for_memory() {
        // Free memory changes all the time; the model is demonstrably running.
        let labels = BTreeMap::from([
            (
                PK_CPU.to_string(),
                www(Reason::NotEnoughMemory { need_mb: 2_048 }),
            ),
            (SMALL_CPU.to_string(), vec![Label::Recommended]),
        ]);
        assert_eq!(
            active_after_test(Some(PK_CPU), &labels, &Catalog::builtin(), &|_| true),
            ActiveDecision::Keep
        );
    }

    #[test]
    fn a_new_download_starts_on_its_best_variant_for_this_pc() {
        let c = catalog_with_directml();
        let pk = c.get("parakeet-tdt-0.6b-v2-int8").unwrap();
        // Labelled Recommended wins.
        let labels = BTreeMap::from([
            (PK_DML.to_string(), vec![Label::Recommended]),
            (PK_CPU.to_string(), vec![]),
        ]);
        assert_eq!(variant_to_activate(pk, &labels, &[]), PK_DML);
        // Otherwise the fastest variant that isn't marked "won't work well".
        let r = |v: &str, p50: u64| VariantResult {
            variant: v.into(),
            metrics: Some(aural_eval::metrics::RunMetrics {
                wer: 0.02,
                words: 0,
                p50_ms: p50,
                p95_ms: p50,
                rtf: 0.1,
            }),
            load_ms: 1_000,
            ram_mb: 800,
            vram_mb: None,
            spread: 1.0,
            passes: 0,
            stability: aural_models::Stability::Stable,
            measured: false,
            error: None,
        };
        let labels = BTreeMap::from([(PK_DML.to_string(), vec![]), (PK_CPU.to_string(), vec![])]);
        assert_eq!(
            variant_to_activate(pk, &labels, &[r(PK_CPU, 300), r(PK_DML, 200)]),
            PK_DML
        );
        let labels = BTreeMap::from([
            (PK_DML.to_string(), www(Reason::NoGpu)),
            (PK_CPU.to_string(), vec![]),
        ]);
        assert_eq!(
            variant_to_activate(pk, &labels, &[r(PK_CPU, 300), r(PK_DML, 200)]),
            PK_CPU
        );
        // Nothing known yet: the processor.
        assert_eq!(variant_to_activate(pk, &BTreeMap::new(), &[]), PK_CPU);
    }

    #[test]
    fn a_new_check_replaces_old_results_only_when_it_completes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ResultsStore::load(&dir.path().join("hardware.json")).unwrap();
        store.reset_for(&laptop(true));
        store.put(VariantResult {
            variant: PK_CPU.into(),
            metrics: None,
            load_ms: 0,
            ram_mb: 0,
            vram_mb: None,
            spread: 0.0,
            passes: 3,
            stability: aural_models::Stability::Stable,
            measured: true,
            error: None,
        });
        let before = store.get(PK_CPU).cloned();
        let mut run = TestRun::default();
        run.results.push(VariantResult {
            variant: SMALL_CPU.into(),
            ..before.clone().unwrap()
        });
        // Cancelled: nothing changes.
        run.commit_if_complete(&mut store, &laptop(true), false);
        assert_eq!(store.get(PK_CPU).cloned(), before);
        assert!(store.get(SMALL_CPU).is_none());
        // Completed: the new run replaces the old one.
        run.commit_if_complete(&mut store, &laptop(true), true);
        assert!(store.get(PK_CPU).is_none());
        assert!(store.get(SMALL_CPU).is_some());
    }

    #[test]
    fn results_from_other_hardware_are_shown_as_expected_not_measured() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ResultsStore::load(&dir.path().join("hardware.json")).unwrap();
        store.reset_for(&laptop(true));
        store.put(VariantResult {
            variant: PK_CPU.into(),
            metrics: Some(aural_eval::metrics::RunMetrics {
                wer: 0.005,
                words: 396,
                p50_ms: 350,
                p95_ms: 500,
                rtf: 0.05,
            }),
            load_ms: 1_800,
            ram_mb: 770,
            vram_mb: None,
            spread: 1.3,
            passes: 3,
            stability: aural_models::Stability::Stable,
            measured: true,
            error: None,
        });
        // Same PC: measured.
        let e = evaluate(&Catalog::builtin(), &store, &laptop(true), 396);
        assert!(e.results.iter().any(|r| r.variant == PK_CPU && r.measured));
        // The graphics card was removed since: not "Measured on your PC" any more.
        let e = evaluate(&Catalog::builtin(), &store, &laptop(false), 396);
        let pk = e.results.iter().find(|r| r.variant == PK_CPU).unwrap();
        assert!(!pk.measured);
    }

    #[test]
    fn the_loaded_model_s_own_memory_counts_as_available() {
        // Parakeet is loaded and holds 770 MB, so free memory is 770 MB lower than it
        // would be without it; that must not make Parakeet itself "not fit".
        assert_eq!(free_for_labels(900, Some(770)), 1_670);
        assert_eq!(free_for_labels(900, None), 900);
    }

    #[test]
    fn a_finished_check_counts_even_without_test_models() {
        // "Check without downloading" on a PC with no model: nothing to measure, but the
        // check happened, so the first-run prompt must not come back.
        let dir = tempfile::tempdir().unwrap();
        let mut store = ResultsStore::load(&dir.path().join("hardware.json")).unwrap();
        assert!(!store.checked());
        TestRun::default().commit_if_complete(&mut store, &laptop(false), true);
        assert!(store.checked());
        // A measurement after a download alone is not a check.
        let mut other = ResultsStore::load(&dir.path().join("other.json")).unwrap();
        other.adopt(&laptop(false));
        assert!(!other.checked());
    }

    #[test]
    fn cancelling_a_check_keeps_measurements_of_new_downloads() {
        let mut jobs = VecDeque::from([
            Job::Measure(PK_CPU.into()),
            Job::FullTest {
                allow_probe_download: true,
            },
            Job::Measure(SMALL_CPU.into()),
        ]);
        assert!(drop_tests(&mut jobs));
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().all(|j| matches!(j, Job::Measure(_))));
        assert!(!drop_tests(&mut jobs));
    }

    #[test]
    fn download_triggers_measurement() {
        let c = catalog_with_directml();
        let pk = c.get("parakeet-tdt-0.6b-v2-int8").unwrap();
        let mut q = MeasureQueue::default();
        q.on_download_complete(pk, &laptop(true));
        // The graphics card first: its result decides quickly whether the model is
        // usable there, and the processor run can be slow for big models.
        assert_eq!(q.pop().as_deref(), Some(PK_DML));
        assert_eq!(q.pop().as_deref(), Some(PK_CPU));
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
