<script lang="ts">
  import { onDestroy } from "svelte";
  import { call, on } from "../lib/api";
  import Pill from "../pill/Pill.svelte";
  import type { AppState, Settings } from "../lib/types";

  let { app, save }: { app: AppState; save: (s: Settings) => Promise<void> } = $props();

  let testing = $state(false);
  let testError = $state<string | null>(null);
  let levels = $state<number[]>([]);
  let off: (() => void) | null = null;

  async function toggleTest() {
    if (testing) return stop();
    testError = null;
    off = await on<number[]>("mic-levels", (l) => (levels = l));
    try {
      await call("mic_test_start");
      testing = true;
    } catch (e) {
      testError = String(e);
      off?.();
      off = null;
    }
  }

  async function stop() {
    testing = false;
    off?.();
    off = null;
    levels = [];
    await call("mic_test_stop");
  }

  onDestroy(() => {
    if (testing) void stop();
  });

  async function choose(e: Event) {
    const value = (e.currentTarget as HTMLSelectElement).value;
    const restart = testing;
    if (restart) await stop();
    await save({ ...app.settings, audio: { device: value === "" ? null : value } });
    if (restart) await toggleTest();
  }

  let defaultName = $derived(app.devices.find((d) => d.is_default)?.name ?? "none found");
  let missing = $derived(
    app.settings.audio.device !== null && !app.devices.some((d) => d.name === app.settings.audio.device),
  );
</script>

<div class="page">
  <h1>Microphone</h1>
  <p class="lede">Aural only opens the microphone while you hold the hotkey or run a test here.</p>

  {#if app.mic_consent === "blocked"}
    <div class="callout" role="alert">
      <div class="text">
        <strong>Windows is blocking microphone access.</strong>
        <div class="muted">Turn on “Microphone access” and “Let desktop apps access your microphone”.</div>
      </div>
      <button class="btn primary" onclick={() => call("open_mic_privacy")}>Open privacy settings</button>
    </div>
  {/if}

  <section class="section">
    <h2>Input</h2>
    <div class="rows">
      <div class="row">
        <div class="text">
          <div class="title">Microphone</div>
          <div class="desc">
            {#if missing}The chosen microphone isn't connected; Aural uses the Windows default until it is.
            {:else}The Windows default follows whatever you pick in Sound settings.{/if}
          </div>
        </div>
        <div class="control">
          <select class="field" aria-label="Microphone" value={app.settings.audio.device ?? ""} onchange={choose}>
            <option value="">Windows default ({defaultName})</option>
            {#each app.devices as d (d.name)}
              <option value={d.name}>{d.name}</option>
            {/each}
            {#if missing}<option value={app.settings.audio.device}>{app.settings.audio.device} (not connected)</option>{/if}
          </select>
        </div>
      </div>
      <div class="row test">
        <div class="text">
          <div class="title">Test</div>
          <div class="desc">
            {#if testError}{testError}
            {:else if testing}Speak — the bars follow your voice exactly as the pill will.
            {:else}See the listening pill react to your microphone.{/if}
          </div>
        </div>
        <div class="control">
          <div class="preview" class:on={testing}>
            <Pill preview={{ state: { state: "listening" }, label: null }} levels={levels} />
          </div>
          <button class="btn" class:primary={!testing} onclick={toggleTest}>{testing ? "Stop" : "Test microphone"}</button>
        </div>
      </div>
    </div>
  </section>
</div>

<style>
  .preview { opacity: 0.35; transition: opacity 160ms ease-out; }
  .preview.on { opacity: 1; }
  .test { min-height: 72px; }
</style>
