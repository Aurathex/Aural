<script lang="ts">
  import { call } from "../lib/api";
  import { DELETE_PHRASE, isDeleteConfirmed } from "../lib/confirm";

  let { close }: { close: () => void } = $props();

  let typed = $state("");
  let working = $state(false);
  let error = $state<string | null>(null);
  let confirmed = $derived(isDeleteConfirmed(typed));
  let cancelButton: HTMLButtonElement | undefined = $state();

  $effect(() => {
    // Safe default: focus lands on Cancel, never on Delete.
    cancelButton?.focus();
  });

  async function confirm(e: SubmitEvent) {
    e.preventDefault();
    if (!confirmed || working) return;
    working = true;
    error = null;
    try {
      await call("delete_aural", { confirmation: typed });
    } catch (err) {
      error = String(err);
      working = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !working) close();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="scrim" role="presentation" onclick={() => !working && close()}></div>
<div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="del-title" aria-describedby="del-body">
  <form onsubmit={confirm}>
    <h2 id="del-title">Are you sure you want to delete Aural?</h2>
    <div id="del-body" class="body">
      <p>This removes Aural from this PC:</p>
      <ul>
        <li>the app itself (it runs the Aural uninstaller),</li>
        <li>every downloaded speech model,</li>
        <li>your settings and the “start with Windows” entry.</li>
      </ul>
      <p>This can't be undone.</p>
    </div>
    <label for="del-input">Type <strong class="phrase">{DELETE_PHRASE}</strong> to confirm</label>
    <input
      id="del-input"
      bind:value={typed}
      autocomplete="off"
      spellcheck="false"
      disabled={working}
      aria-invalid={typed.length > 0 && !confirmed}
    />
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="buttons">
      <button type="button" class="btn" bind:this={cancelButton} onclick={close} disabled={working}>Cancel</button>
      <button type="submit" class="btn danger" disabled={!confirmed || working}>
        {working ? "Deleting…" : "Delete Aural"}
      </button>
    </div>
  </form>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.45); }
  .dialog {
    position: fixed;
    top: 50%;
    left: 50%;
    width: min(460px, calc(100vw - 48px));
    transform: translate(-50%, -50%);
    padding: 24px;
    border-radius: 12px;
    background: var(--bg);
    border: 1px solid var(--line);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.3);
  }
  h2 { margin: 0 0 12px; font: 600 18px/1.3 var(--font-display); }
  .body { color: var(--ink-2); }
  .body p { margin: 0 0 8px; }
  .body ul { margin: 0 0 8px; padding-left: 18px; }
  label { display: block; margin: 16px 0 6px; font-weight: 600; }
  .phrase { font-family: "Cascadia Mono", Consolas, monospace; letter-spacing: 0.02em; }
  input {
    width: 100%;
    height: 34px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    font-family: "Cascadia Mono", Consolas, monospace;
    user-select: text;
  }
  input[aria-invalid="true"] { border-color: var(--danger); }
  .error { color: var(--danger); margin: 8px 0 0; }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; margin-top: 20px; }
</style>
