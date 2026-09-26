<script lang="ts">
  import { onMount } from "svelte";
  import Logo from "../lib/Logo.svelte";
  import Bars from "../lib/Bars.svelte";
  import { on } from "../lib/api";
  import type { PillView } from "../lib/types";

  // `preview` lets the settings window show the same pill (microphone test).
  let {
    preview = null,
    levels: previewLevels = null,
  }: { preview?: PillView | null; levels?: number[] | null } = $props();

  let view = $state<PillView>({ state: { state: "hidden" }, label: null });
  let levels = $state<number[]>([]);

  let current = $derived(preview ?? view);
  let kind = $derived(current.state.state);
  let bands = $derived(previewLevels ?? levels);

  onMount(() => {
    if (preview) return;
    const offs = [
      on<PillView>("pill", (v) => (view = v)),
      on<number[]>("levels", (l) => (levels = l)),
    ];
    return () => offs.forEach((p) => p.then((off) => off()));
  });
</script>

<div class="pill {kind}" role="status" aria-live="polite">
  <span class="mark"><Logo size={18} weight={6} /></span>
  <div class="field">
    {#if kind === "error"}
      <span class="label">{current.label}</span>
    {:else if kind === "success"}
      <span class="line"></span>
    {:else}
      <Bars levels={bands} mode={kind === "listening" ? "live" : "dots"} />
      {#if kind === "processing"}<span class="sweep"></span>{/if}
    {/if}
  </div>
  <span class="sr">
    {kind === "listening" ? "Listening" : kind === "processing" ? "Transcribing" : kind === "success" ? "Text inserted" : (current.label ?? "")}
  </span>
</div>

<style>
  .pill {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    width: 176px;
    height: 36px;
    padding: 0 16px 0 14px;
    border-radius: 18px;
    background: #0b0b0b;
    color: #ffffff;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.1);
    opacity: 1;
    transform: translateY(0) scale(1);
    transition: opacity 160ms ease-out, transform 160ms ease-out;
    overflow: hidden;
  }
  .pill.hidden {
    opacity: 0;
    transform: translateY(4px) scale(0.96);
  }
  .mark { display: flex; flex: none; }
  .field {
    position: relative;
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    height: 20px;
  }
  /* Processing: a soft highlight travels across the settled dots. */
  /* Dims every dot except a clear band that travels left to right (transform only). */
  .sweep {
    position: absolute;
    top: 0;
    bottom: 0;
    left: -100%;
    width: 300%;
    background: linear-gradient(
      90deg,
      rgba(11, 11, 11, 0.7) 0%,
      rgba(11, 11, 11, 0.7) 44%,
      rgba(11, 11, 11, 0) 50%,
      rgba(11, 11, 11, 0.7) 56%,
      rgba(11, 11, 11, 0.7) 100%
    );
    animation: sweep 900ms linear infinite;
  }
  @keyframes sweep {
    from { transform: translateX(-33.333%); }
    to { transform: translateX(33.333%); }
  }
  /* Success: the dots join into one line. */
  .line {
    display: block;
    width: 100%;
    height: 2px;
    border-radius: 1px;
    background: #fff;
    transform-origin: center;
    animation: join 220ms ease-out both;
  }
  @keyframes join {
    from { transform: scaleX(0.15); opacity: 0.4; }
    to { transform: scaleX(1); opacity: 1; }
  }
  /* Error: a short grey label and one small nudge. */
  .label {
    font: 500 11.5px/1 "Segoe UI Variable Text", "Segoe UI", sans-serif;
    color: #a3a3a3;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .error { animation: nudge 260ms ease-out 1; }
  @keyframes nudge {
    0%, 100% { transform: translateX(0); }
    30% { transform: translateX(-2px); }
    60% { transform: translateX(2px); }
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  @media (prefers-reduced-motion: reduce) {
    .sweep { animation: none; opacity: 0.4; }
    .line, .error { animation: none; }
  }
</style>
