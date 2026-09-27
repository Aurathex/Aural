<script lang="ts">
  import {
    accuracyText,
    labelText,
    measuredText,
    memoryText,
    placeText,
    speedText,
    startupText,
    variantBackend,
  } from "./copy";
  import { formatBytes, percent } from "./format";
  import type { AppState, Label, ModelStatus, VariantResult } from "./types";

  let {
    m,
    app,
    busy,
    act,
  }: {
    m: ModelStatus;
    app: AppState;
    busy: string | null;
    act: (cmd: string, id: string) => void;
  } = $props();

  const installed = $derived(m.state.kind === "installed" || m.state.kind === "active");
  const result = (v: string): VariantResult | undefined => app.results.find((r) => r.variant === v);
  const labels = (v: string): Label[] => app.labels[v] ?? [];
  const inUse = (v: string) => m.state.kind === "active" && app.settings.stt.active_variant === v;
  const pct = (x: number) => `${(x * 100).toFixed(1)}%`;
</script>

<div class="model">
  <div class="head">
    <div class="text">
      <div class="name">{m.name}</div>
      <div class="desc">{m.description}</div>
      <div class="meta num">
        {formatBytes(m.size_bytes)} download · {m.license_id}
        {#if !m.compatible}<span class="warn">· Needs about {Math.round(m.min_ram_mb / 1024)} GB of memory</span>{/if}
      </div>
      {#if m.state.kind === "downloading"}
        {@const p = percent(m.state.downloaded, m.state.total)}
        <div class="progress" role="progressbar" aria-valuenow={p} aria-valuemin="0" aria-valuemax="100" aria-label="Downloading {m.name}">
          <span style:transform="scaleX({p / 100})"></span>
        </div>
        <div class="meta num">{formatBytes(m.state.downloaded)} of {formatBytes(m.state.total)} · {p}%</div>
      {/if}
    </div>
    <div class="actions">
      {#if m.state.kind === "installed"}
        <button class="btn quiet" disabled={busy === m.id} onclick={() => act("remove_model", m.id)}>Remove</button>
      {:else if m.state.kind === "downloading"}
        <button class="btn" onclick={() => act("cancel_download", m.id)}>Cancel</button>
      {:else if m.state.kind === "available"}
        {@const recommended = m.variants.some((v) => labels(v).some((l) => l.label === "recommended"))}
        <button class="btn" class:primary={recommended || m.recommended} disabled={busy === m.id} onclick={() => act("download_model", m.id)}>
          Download
        </button>
      {/if}
    </div>
  </div>

  <ul class="variants" aria-label="Where {m.name} can run">
    {#each m.variants as v (v)}
      {@const r = result(v)}
      {@const ls = labels(v)}
      <li class="variant">
        <div class="vtext">
          <div class="where">
            {placeText(variantBackend(v))}
            {#each ls as l, i (i)}
              {@const t = labelText(l)}
              <span class="chip" class:strong={l.label === "recommended"} class:bad={l.label === "wont_work_well"} title={t.hint}>{t.name}<span class="sr">: {t.hint}</span></span>
            {/each}
          </div>
          {#each ls as l, i (i)}
            {#if l.label === "wont_work_well"}<div class="why">{labelText(l).hint}</div>{/if}
          {/each}
          {#if r?.metrics}
            <div class="facts">
              <span>{accuracyText(r.metrics.wer)}</span>
              <span>{speedText(r.metrics.p50_ms)}</span>
              <span>{memoryText(r.ram_mb, r.vram_mb)}</span>
              {#if r.load_ms > 0}<span>{startupText(r.load_ms)}</span>{/if}
            </div>
            <div class="source">{measuredText(r.measured)}</div>
          {:else if !ls.length}
            <div class="source">Not tried on this PC yet.{installed ? "" : " Download it to try it."}</div>
          {/if}
        </div>
        <div class="vactions">
          {#if inUse(v)}
            <span class="badge">In use</span>
          {:else if installed && !ls.some((l) => l.label === "wont_work_well")}
            <button class="btn primary" disabled={busy === v} onclick={() => act("use_variant", v)}>Use</button>
          {/if}
        </div>
      </li>
    {/each}
  </ul>

  <details class="tech">
    <summary>Technical details</summary>
    <dl class="num">
      <dt>Model</dt><dd>{m.id} · {m.runtime} · {m.precision}</dd>
      {#each m.variants as v (v)}
        {@const r = result(v)}
        <dt>{variantBackend(v)}</dt>
        <dd>
          {#if r?.metrics}
            WER {pct(r.metrics.wer)} over {r.metrics.words || "reference"} words · p50 {Math.round(r.metrics.p50_ms)} ms · p95
            {Math.round(r.metrics.p95_ms)} ms · RTF {r.metrics.rtf.toFixed(3)} · load {r.load_ms} ms · RAM {r.ram_mb} MB · VRAM
            {r.vram_mb ?? "not measured"}{r.vram_mb === null ? "" : " MB"} · {r.measured ? `measured, ${r.passes} passes, spread ${r.spread.toFixed(2)}` : "estimated"}
            {#if r.stability.state === "unstable"} · unstable: {r.stability.reason}{/if}
            {#if r.error} · error: {r.error}{/if}
          {:else if r?.error}
            error: {r.error}
          {:else}
            no data yet
          {/if}
        </dd>
      {/each}
      <dt>License</dt><dd>{m.attribution}</dd>
    </dl>
  </details>
</div>

<style>
  .model { padding: 16px 0; border-bottom: 1px solid var(--line); }
  .head { display: flex; gap: 24px; align-items: flex-start; }
  .text { flex: 1; min-width: 0; }
  .name { font-weight: 600; font-size: 14px; }
  .desc { color: var(--ink-2); margin-top: 2px; }
  .meta { color: var(--ink-3); margin-top: 6px; font-size: 12px; }
  .warn { color: var(--ink); font-weight: 600; }
  .actions, .vactions { flex: none; display: flex; gap: 6px; align-items: center; }
  .variants { list-style: none; margin: 10px 0 0; padding: 0; }
  .variant {
    display: flex;
    gap: 16px;
    align-items: flex-start;
    padding: 10px 12px;
    margin-top: 6px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .vtext { flex: 1; min-width: 0; }
  .where { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; font-weight: 600; }
  .chip {
    font-size: 11px;
    font-weight: 600;
    padding: 1px 8px;
    border-radius: 10px;
    border: 1px solid var(--line);
    color: var(--ink-2);
  }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .chip.strong { background: var(--invert-bg); color: var(--invert-ink); border-color: var(--invert-bg); }
  .chip.bad { border-style: dashed; color: var(--ink); }
  .why { margin-top: 4px; color: var(--ink); }
  .facts { display: flex; flex-direction: column; gap: 1px; margin-top: 4px; color: var(--ink-2); }
  .source { margin-top: 4px; color: var(--ink-3); font-size: 12px; }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 12px;
    border-radius: 6px;
    background: var(--field);
    font-weight: 600;
  }
  .badge::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--ink); }
  .progress {
    position: relative;
    height: 4px;
    margin-top: 10px;
    border-radius: 2px;
    background: var(--line);
    overflow: hidden;
  }
  .progress span {
    position: absolute;
    inset: 0;
    background: var(--ink);
    transform-origin: left;
    transition: transform 200ms linear;
  }
  details.tech { margin-top: 8px; font-size: 12px; color: var(--ink-3); }
  details.tech summary { cursor: pointer; width: fit-content; }
  dl { display: grid; grid-template-columns: max-content 1fr; gap: 2px 12px; margin: 6px 0 0; }
  dt { font-weight: 600; }
  dd { margin: 0; }
</style>
