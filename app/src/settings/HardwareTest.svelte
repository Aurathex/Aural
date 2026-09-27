<script lang="ts">
  import { call } from "../lib/api";
  import { backgroundMeasuring, labelText, placeText, recheckChoices, stepText, variantBackend } from "../lib/copy";
  import type { AppState } from "../lib/types";

  let { app, update }: { app: AppState; update: (next: AppState) => void } = $props();
  let error = $state<string | null>(null);

  const t = $derived(app.hardware_test);
  const modelName = (variant: string) => app.models.find((m) => m.id === variant.split("@")[0])?.name;

  const step = $derived.by(() => {
    const s = t.step;
    if (!s) return "Getting ready…";
    return stepText(s, s.step === "measuring" ? modelName(s.variant) : undefined);
  });

  const recommended = $derived(
    Object.entries(app.labels).find(([, ls]) => ls.some((l) => l.label === "recommended"))?.[0],
  );

  const summary = $derived.by(() => {
    if (!recommended) {
      return "Download a model and Aural will try it on your PC to show how well it works here.";
    }
    const name = modelName(recommended) ?? "a model";
    const where = placeText(variantBackend(recommended)).replace("On", "on");
    return `For this PC, Aural recommends ${name} ${where}. ${labelText({ label: "recommended" }).hint}.`;
  });

  const background = $derived(
    backgroundMeasuring(t, t.step?.step === "measuring" ? modelName(t.step.variant) : undefined),
  );

  async function start(allowDownload: boolean) {
    error = null;
    try {
      update(await call<AppState>("start_hardware_test", { allowProbeDownload: allowDownload }));
    } catch (e) {
      error = String(e);
    }
  }

  const cancel = () => call("cancel_hardware_test");
</script>

<section class="card" aria-labelledby="hwtest-title">
  {#if t.running}
    <h2 id="hwtest-title">Checking your PC</h2>
    <p class="live" aria-live="polite">{step}</p>
    {#if t.total > 0}
      <div class="steps num" aria-hidden="true">{t.done + 1} of {t.total}</div>
    {/if}
    <p class="muted">You can keep dictating while this runs.</p>
    <div class="buttons"><button class="btn" onclick={cancel}>Cancel</button></div>
  {:else if !t.tested}
    <h2 id="hwtest-title">Find the right model for this PC</h2>
    <p>
      Aural will check what your PC can handle and try a couple of small voice models to see how fast they run. This
      takes a minute or two and downloads about 75 MB. Nothing leaves your PC.
    </p>
    <div class="buttons">
      <button class="btn primary" onclick={() => start(true)}>Check my PC</button>
      <button class="btn quiet" onclick={() => start(false)}>Check without downloading</button>
    </div>
  {:else if t.stale}
    <h2 id="hwtest-title">Your PC has changed</h2>
    <p>Something changed since the last check, such as a new graphics card or driver. Check again to update the advice below.</p>
    <div class="buttons">
      {#each recheckChoices(t) as c, i (c.label)}
        <button class="btn" class:primary={i === 0} class:quiet={i > 0} onclick={() => start(c.allowDownload)}>{c.label}</button>
      {/each}
    </div>
  {:else}
    <h2 id="hwtest-title">What suits this PC</h2>
    <p aria-live="polite">{summary}</p>
    <div class="buttons">
      {#each recheckChoices(t) as c (c.label)}
        <button class="btn quiet" onclick={() => start(c.allowDownload)}>{c.label}</button>
      {/each}
    </div>
  {/if}
  {#if background}<p class="live" aria-live="polite">{background}</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</section>

<style>
  .card {
    margin: 0 0 16px;
    padding: 14px 16px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  h2 { margin: 0 0 6px; font-size: 14px; font-weight: 600; }
  p { margin: 0 0 10px; color: var(--ink-2); }
  .live { color: var(--ink); font-weight: 600; }
  .steps { color: var(--ink-3); font-size: 12px; margin: -6px 0 8px; }
  .buttons { display: flex; gap: 8px; }
  .error { color: var(--ink); font-weight: 600; margin-top: 8px; }
</style>
