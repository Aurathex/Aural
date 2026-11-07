<script lang="ts">
  import { call } from "../lib/api";
  import type { TouchupPreview } from "../lib/types";
  let { preview, update }: { preview: TouchupPreview; update: (p: TouchupPreview) => void } = $props();
  let dialog: HTMLDialogElement;
  let error = $state<{ id: number; message: string } | null>(null);
  let message = $derived(error?.id === preview.id ? error.message : preview.error);
  let busy = $derived(preview.phase === "applying");
  $effect(() => {
    if (!dialog) return;
    if (preview.phase && !dialog.open) dialog.showModal();
    if (!preview.phase && dialog.open) dialog.close();
  });
  async function close() {
    if (busy) return;
    const id = preview.id;
    error = null;
    await call("cancel_touchup", { id });
    update({ id: id + 1, phase: "", original: "", target: "", suggestion: "", error: null });
  }
  async function apply() {
    const original = { ...preview };
    const id = original.id;
    error = null;
    update({ ...original, phase: "applying" });
    try { update(await call<TouchupPreview>("apply_touchup", { id })); }
    catch (e) {
      if (preview.id === id) error = { id, message: String(e) };
      update({ ...original, phase: "error" });
    }
  }
</script>

<dialog bind:this={dialog} aria-labelledby="touchup-title" oncancel={(e) => { e.preventDefault(); void close(); }}>
  <h2 id="touchup-title">Aural AI touch-up</h2>
  <p>Review the local writing helper’s changes before replacing the original selection.</p>
  {#if preview.target}<p><strong>Original window:</strong> {preview.target}</p>{/if}
  {#if preview.original}<h3>Original</h3><pre>{preview.original}</pre>{/if}
  {#if preview.phase === "working"}<p role="status">Fixing grammar, spelling and wording…</p>{/if}
  {#if preview.suggestion}<h3>Suggested revision</h3><pre>{preview.suggestion}</pre>{/if}
  {#if message}<p role="status" class="message">{message}</p>{/if}
  {#if preview.phase === "review"}<p class="hint">Apply returns to the original app and checks that the same text is still selected. Review facts and meaning before applying.</p>{/if}
  <div class="actions">
    <button class="btn" disabled={busy} onclick={close}>{preview.phase === "done" ? "Close" : "Cancel"}</button>
    {#if preview.phase === "review" || busy}
      <button class="btn primary" disabled={busy} onclick={apply}>{busy ? "Applying…" : "Apply to original selection"}</button>
    {/if}
  </div>
</dialog>

<style>
  dialog { width: min(680px, calc(100vw - 40px)); max-height: calc(100vh - 48px); padding: 28px; border: 1px solid var(--line); border-radius: 14px; color: var(--ink); background: var(--paper, var(--side)); box-shadow: 0 16px 64px #0003; }
  dialog::backdrop { background: #0005; }
  h2 { margin: 0 0 10px; font: 600 22px var(--font-display); }
  h3 { margin: 20px 0 8px; font-size: 13px; }
  p { color: var(--ink-2); font-size: 13px; line-height: 1.5; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; font: 14px/1.6 var(--font); padding: 14px; border: 1px solid var(--line); border-radius: 8px; max-height: 24vh; overflow: auto; }
  .message { color: var(--ink); }
  .actions { display: flex; justify-content: flex-end; gap: 10px; margin-top: 22px; }
</style>
