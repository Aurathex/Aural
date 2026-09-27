use anyhow::{Context, Result};
use aural_bench::audio::load_wav_16k_mono;
use aural_bench::cli::{build_engine, Cli};
use aural_bench::mem::{current_working_set_mb, peak_working_set_mb};
use aural_bench::runner::{parse_corpus_tsv, run, summarize, threads_label, CorpusClip};
use clap::Parser;
use std::time::Instant;

fn default_threads() -> usize {
    let logical = std::thread::available_parallelism().map_or(4, |n| n.get());
    // Approximate physical cores as half the logical count on SMT machines.
    (logical / 2).saturating_sub(1).clamp(1, 8)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let threads = cli.threads.unwrap_or_else(default_threads);

    let tsv = std::fs::read_to_string(&cli.corpus)
        .with_context(|| format!("reading {}", cli.corpus.display()))?;
    let base = cli.corpus.parent().unwrap_or(std::path::Path::new("."));
    let corpus: Vec<CorpusClip> = parse_corpus_tsv(&tsv, base)?
        .into_iter()
        .map(|e| Ok((e.id, load_wav_16k_mono(&e.wav)?, e.reference)))
        .collect::<Result<_>>()?;
    // Decoded corpus audio is resident for the whole run; report model RAM net of it.
    let baseline_mb = current_working_set_mb()?;

    let load_start = Instant::now();
    let mut engine = build_engine(cli.engine, &cli.model, cli.backend, threads)?;
    let load_ms = load_start.elapsed().as_secs_f64() * 1000.0;

    let results = run(engine.as_mut(), &corpus, cli.warmup)?;
    let summary = summarize(&results);
    let peak_mb = peak_working_set_mb()?;
    let engine_ram_mb = (peak_mb - baseline_mb).max(0.0);
    let threads_used = engine.threads();
    // Graphics-card memory this process holds with the model still loaded.
    let gpu_memory_mb = aural_platform::gpu::process_gpu_memory_mb();

    println!("| engine | backend | threads | clips | WER | p50 ms | p95 ms | mean RTF | load ms | engine RAM MB |");
    println!("|---|---|---|---|---|---|---|---|---|---|");
    println!(
        "| {} | {} | {} | {} | {:.2}% | {:.0} | {:.0} | {:.3} | {:.0} | {:.0} |",
        engine.label(),
        engine.backend_used(),
        threads_label(threads_used),
        summary.clips,
        summary.corpus_wer * 100.0,
        summary.p50_ms,
        summary.p95_ms,
        summary.mean_rtf,
        load_ms,
        engine_ram_mb
    );

    let report = serde_json::json!({
        "engine": engine.label(),
        "backend_requested": format!("{:?}", cli.backend),
        "backend_used": engine.backend_used(),
        "threads": threads_used,
        "load_ms": load_ms,
        "peak_working_set_mb": peak_mb,
        "corpus_baseline_mb": baseline_mb,
        "engine_ram_mb": engine_ram_mb,
        "gpu_memory_mb": gpu_memory_mb,
        "summary": summary,
        "clips": results,
    });
    std::fs::write(&cli.out, serde_json::to_string_pretty(&report)?)
        .with_context(|| format!("writing {}", cli.out.display()))?;
    Ok(())
}
