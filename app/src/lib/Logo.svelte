<script lang="ts">
  // The Aural mark: a voice wave that loses amplitude, straightens and resolves into a
  // point. Draws in the current text colour.
  let { size = 20, weight = 5 }: { size?: number; weight?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 64 64" fill="none" aria-hidden="true">
  <path
    d="M6 32 C 10 13, 17 13, 20 32 S 29 45, 32 32 S 38 26, 40 32 L 48 32"
    stroke="currentColor"
    stroke-width={weight}
    stroke-linecap="round"
    stroke-linejoin="round"
  />
  <circle cx="55.5" cy="32" r={weight * 0.8} fill="currentColor" />
</svg>
