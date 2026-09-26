<script lang="ts">
  import { onDestroy } from "svelte";
  import { BAR_COUNT, barHeights, smooth } from "./bars";

  // Real microphone levels in (12 bands, 0..1), smoothed at display rate.
  // mode "live" follows the levels, "dots" settles every bar to the idle dot.
  let {
    levels = [],
    mode = "live",
    height = 20,
  }: { levels?: number[]; mode?: "live" | "dots"; height?: number } = $props();

  const MIN = 2;
  let shown = $state<number[]>(new Array(BAR_COUNT).fill(0));
  let last = performance.now();
  let frame = 0;

  function tick(now: number) {
    const target = mode === "live" ? levels : new Array(BAR_COUNT).fill(0);
    shown = smooth(shown, target, now - last);
    last = now;
    frame = requestAnimationFrame(tick);
  }
  frame = requestAnimationFrame(tick);
  onDestroy(() => cancelAnimationFrame(frame));

  let heights = $derived(barHeights(shown, height, MIN));
</script>

<div class="bars" style:height="{height}px" aria-hidden="true">
  {#each heights as h, i (i)}
    <i style:transform="scaleY({h / height})" style:--i={i}></i>
  {/each}
</div>

<style>
  .bars {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  i {
    display: block;
    width: 2px;
    height: 100%;
    border-radius: 1px;
    background: currentColor;
    transform-origin: center;
    will-change: transform;
  }
</style>
