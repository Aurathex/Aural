<script lang="ts">
  import { call } from "../lib/api";
  import Logo from "../lib/Logo.svelte";
  import DeleteDialog from "./DeleteDialog.svelte";
  import type { AppState } from "../lib/types";

  let { app }: { app: AppState } = $props();
  let deleting = $state(false);
  // Matches the installer's copyright line (tauri.conf.json).
  const year = 2026;
</script>

<div class="page">
  <div class="brand">
    <span class="mark"><Logo size={40} /></span>
    <div>
      <h1>Aural</h1>
      <div class="maker">by Aurathex</div>
      <div class="muted num">Version {app.version}</div>
    </div>
  </div>

  <section class="section">
    <h2>Privacy</h2>
    <div class="rows">
      <div class="row"><div class="text"><div class="title">Your voice stays on this PC</div><div class="desc">Speech is transcribed locally. Audio is kept in memory only while you dictate and is never saved or uploaded.</div></div></div>
      <div class="row"><div class="text"><div class="title">No account, no telemetry</div><div class="desc">Aural sends no usage data anywhere. Your history and statistics stay on this PC. It only uses the network when you download a model.</div></div></div>
      <div class="row"><div class="text"><div class="title">Clipboard</div><div class="desc">Dictated text is pasted through the clipboard, kept out of Windows clipboard history, and your previous clipboard text is put back.</div></div></div>
      <div class="row">
        <div class="text"><div class="title">Data folder</div><div class="desc num">{app.data_dir}</div></div>
        <div class="control"><button class="btn" onclick={() => call("open_data_folder")}>Open</button></div>
      </div>
    </div>
  </section>

  <section class="section">
    <h2>Credits</h2>
    <div class="credits">
      {#each app.models as m (m.id)}<p>{m.attribution}</p>{/each}
      <p>Aural is free for personal and other noncommercial use under the PolyForm Strict License 1.0.0, which does not allow modifying or redistributing it. Commercial use needs a license from Aurathex. The license and third-party notices are in the install folder (LICENSE.md, THIRD_PARTY_NOTICES.md).</p>
    </div>
  </section>

  <section class="section">
    <h2>Aurathex</h2>
    <div class="rows">
      <div class="row">
        <div class="text">
          <div class="title">Made by Aurathex</div>
          <div class="desc">Aural is designed and built by Aurathex. © {year} Aurathex. All rights reserved.</div>
        </div>
      </div>
    </div>
  </section>

  <section class="section danger-zone">
    <h2>Danger zone</h2>
    <div class="rows">
      <div class="row">
        <div class="text">
          <div class="title">Delete Aural</div>
          <div class="desc">Uninstall the app and remove all downloaded models and settings from this PC.</div>
        </div>
        <div class="control"><button class="btn danger-outline" onclick={() => (deleting = true)}>Delete Aural…</button></div>
      </div>
    </div>
  </section>
</div>

{#if deleting}<DeleteDialog close={() => (deleting = false)} />{/if}

<style>
  .brand { display: flex; align-items: center; gap: 14px; margin-bottom: 28px; }
  .brand h1 { margin: 0; }
  .maker { font: 600 13px/1.3 var(--font-display); letter-spacing: 0.02em; color: var(--ink); }
  .mark {
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    border-radius: 14px;
    background: #0b0b0b;
    color: #fff;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.1);
  }
  .credits { color: var(--ink-2); font-size: 12px; }
  .credits p { margin: 0 0 6px; user-select: text; }
  .danger-outline { color: var(--danger); border-color: var(--danger); }
  .danger-outline:hover:not(:disabled) { background: var(--danger); color: var(--danger-ink); }
</style>
