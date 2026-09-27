<script lang="ts">
  import { call } from "../lib/api";
  import { formatBytes, percent } from "../lib/format";
  import type { AppState, ModelStatus } from "../lib/types";

  let { app, update }: { app: AppState; update: (next: AppState) => void } = $props();
  let busy = $state<string | null>(null);
  let error = $state<string | null>(null);

  async function act(cmd: string, id: string) {
    error = null;
    busy = id;
    try {
      update(await call<AppState>(cmd, { id }));
    } catch (e) {
      error = String(e);
    } finally {
      busy = null;
    }
  }

  function engineLine(a: AppState): string {
    const e = a.engine;
    if (e.state === "ready") return `Ready · ${a.models.find((m) => m.id === e.model)?.name ?? e.label} on CPU`;
    if (e.state === "loading") return "Loading model…";
    if (e.state === "error") return `Could not start the model: ${e.message}`;
    return "No model yet — download one to start dictating.";
  }

  const ram = (mb: number) => `${(mb / 1024).toFixed(1)} GB RAM`;
  const cancel = (m: ModelStatus) => call("cancel_download", { id: m.id });
</script>

<div class="page">
  <h1>Models</h1>
  <p class="lede">Speech is recognised on this PC. Models download once from Hugging Face and are checked before use.</p>

  <div class="summary">
    <span class="dot" class:ready={app.engine.state === "ready"} class:busy={app.engine.state === "loading"}></span>
    <span>{engineLine(app)}</span>
    <span class="hw num">This PC: {ram(app.hardware.total_ram_mb)} · {app.hardware.logical_cores} threads{app.hardware.avx2 ? " · AVX2" : ""}</span>
  </div>

  {#if error}<div class="callout" role="alert"><div class="text">{error}</div></div>{/if}

  <div class="rows">
    {#each app.models as m (m.id)}
      <div class="model">
        <div class="text">
          <div class="name">
            {m.name}
            {#if m.recommended}<span class="tag strong">Recommended for this PC</span>{/if}
          </div>
          <div class="desc">{m.description}</div>
          <div class="meta num">
            {formatBytes(m.size_bytes)} · {m.license_id}
            {#if !m.compatible}<span class="warn">· Needs {ram(m.min_ram_mb)}</span>{/if}
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
          {#if m.state.kind === "active"}
            <span class="badge">In use</span>
          {:else if m.state.kind === "installed"}
            <button class="btn quiet" disabled={busy === m.id} onclick={() => act("remove_model", m.id)}>Remove</button>
            <button class="btn primary" disabled={busy === m.id} onclick={() => act("use_model", m.id)}>Use</button>
          {:else if m.state.kind === "downloading"}
            <button class="btn" onclick={() => cancel(m)}>Cancel</button>
          {:else}
            <button class="btn" class:primary={m.recommended} disabled={busy === m.id} onclick={() => act("download_model", m.id)}>
              Download
            </button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
  <p class="foot muted">Removing a model frees its disk space; you can download it again at any time. The model in use can't be removed — choose another first.</p>
</div>

<style>
  .summary {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 0 16px;
    padding: 10px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .summary { flex-wrap: wrap; row-gap: 2px; }
  .summary .hw { flex-basis: 100%; padding-left: 18px; color: var(--ink-3); font-size: 12px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; border: 1.5px solid var(--ink-3); flex: none; }
  .dot.ready { background: var(--ink); border-color: var(--ink); }
  .dot.busy { border-color: var(--ink); animation: pulse 1s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.3; } }
  .model {
    display: flex;
    gap: 24px;
    align-items: flex-start;
    padding: 16px 0;
    border-bottom: 1px solid var(--line);
  }
  .text { flex: 1; min-width: 0; }
  .name { display: flex; align-items: center; gap: 10px; font-weight: 600; font-size: 14px; }
  .desc { color: var(--ink-2); margin-top: 2px; }
  .meta { color: var(--ink-3); margin-top: 6px; font-size: 12px; }
  .warn { color: var(--ink); font-weight: 600; }
  .tag {
    font-size: 11px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 10px;
    border: 1px solid var(--line);
    color: var(--ink-2);
  }
  .tag.strong { background: var(--invert-bg); color: var(--invert-ink); border-color: var(--invert-bg); }
  .actions { flex: none; display: flex; gap: 6px; align-items: center; padding-top: 2px; }
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
  .foot { margin-top: 16px; font-size: 12px; }
</style>
