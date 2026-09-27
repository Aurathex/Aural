<script lang="ts">
  import { call } from "../lib/api";
  import { hardwareText, placeText, variantBackend } from "../lib/copy";
  import ModelRow from "../lib/ModelRow.svelte";
  import HardwareTest from "./HardwareTest.svelte";
  import type { AppState } from "../lib/types";

  let { app, update }: { app: AppState; update: (next: AppState) => void } = $props();
  let busy = $state<string | null>(null);
  let error = $state<string | null>(null);

  async function act(cmd: string, id: string) {
    error = null;
    busy = id;
    try {
      const next = await call<AppState | undefined>(cmd, { id });
      if (next) update(next);
    } catch (e) {
      error = String(e);
    } finally {
      busy = null;
    }
  }

  function engineLine(a: AppState): string {
    const e = a.engine;
    const where = a.settings.stt.active_variant
      ? placeText(variantBackend(a.settings.stt.active_variant)).replace("On", "on")
      : "";
    if (e.state === "ready") return `Ready · ${a.models.find((m) => m.id === e.model)?.name ?? e.label} ${where}`;
    if (e.state === "loading") return "Getting the model ready…";
    if (e.state === "error") return "The model couldn't start. Choose another one below, or check your PC again.";
    return "No model yet. Download one to start dictating.";
  }
</script>

<div class="page">
  <h1>Models</h1>
  <p class="lede">Speech is turned into text on this PC. Models download once from Hugging Face and are checked before use.</p>

  <div class="summary">
    <span class="dot" class:ready={app.engine.state === "ready"} class:busy={app.engine.state === "loading"}></span>
    <span>{engineLine(app)}</span>
    <span class="hw">{hardwareText(app.hardware)}</span>
    {#if app.engine.state === "error"}
      <details class="tech"><summary>Technical details</summary>{app.engine.message}</details>
    {/if}
  </div>

  <HardwareTest {app} {update} />

  {#if error}
    <div class="callout" role="alert">
      <div class="text">That didn't work. Please try again.</div>
      <details class="tech"><summary>Technical details</summary>{error}</details>
    </div>
  {/if}

  <div class="rows">
    {#each app.models as m (m.id)}
      <ModelRow {m} {app} {busy} {act} />
    {/each}
  </div>
  <p class="foot muted">Removing a model frees its disk space; you can download it again at any time. The model in use can't be removed. Choose another first.</p>
</div>

<style>
  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    row-gap: 2px;
    margin: 0 0 16px;
    padding: 10px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .summary .hw { flex-basis: 100%; padding-left: 18px; color: var(--ink-3); font-size: 12px; }
  .summary details.tech { flex-basis: 100%; padding-left: 18px; color: var(--ink-3); font-size: 12px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; border: 1.5px solid var(--ink-3); flex: none; }
  .dot.ready { background: var(--ink); border-color: var(--ink); }
  .dot.busy { border-color: var(--ink); animation: pulse 1s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.3; } }
  .foot { margin-top: 16px; font-size: 12px; }
</style>
