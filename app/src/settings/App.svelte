<script lang="ts">
  import { onMount } from "svelte";
  import { call, inApp, on } from "../lib/api";
  import Logo from "../lib/Logo.svelte";
  import { latestTouchup } from "../lib/touchup-state";
  import General from "./General.svelte";
  import Microphone from "./Microphone.svelte";
  import Models from "./Models.svelte";
  import About from "./About.svelte";
  import Writing from "./Writing.svelte";
  import Touchup from "./Touchup.svelte";
  import History from "./History.svelte";
  import type { AppState, HwTestStatus, ProgressEvent, Settings } from "../lib/types";

  type Page = "general" | "microphone" | "models" | "writing" | "history" | "about";
  const pages: { id: Page; label: string }[] = [
    { id: "general", label: "General" },
    { id: "microphone", label: "Microphone" },
    { id: "models", label: "Models" },
    { id: "writing", label: "Writing" },
    { id: "history", label: "History" },
    { id: "about", label: "About" },
  ];

  let app = $state<AppState | null>(null);
  let page = $state<Page>("general");
  let saveError = $state<string | null>(null);
  let deleted = $state(false);
  let loadError = $state(false);
  let saving = $state(false);

  function applyProgress(p: ProgressEvent) {
    const m = app?.models.find((x) => x.id === p.id);
    if (m) m.state = { kind: "downloading", downloaded: p.downloaded, total: p.total };
  }

  let firstRun = true;

  // The window can load before the backend has finished starting; the first state
  // (from any source) decides whether to open on Models.
  function receive(s: AppState) {
    loadError = false;
    app = { ...s, touchup: latestTouchup(app?.touchup, s.touchup) };
    if (firstRun) {
      firstRun = false;
      // First start: the Models page, where the hardware test offers itself.
      const noModel = !s.models.some((m) => m.state.kind === "active" || m.state.kind === "installed");
      if (noModel || !s.hardware_test.tested) page = "models";
      // Design preview only: ?page=models opens a page directly for screenshots.
      const asked = inApp ? null : new URLSearchParams(location.search).get("page");
      if (asked && pages.some((p) => p.id === asked)) page = asked as Page;
    }
  }

  async function refresh(attempts = 1) {
    for (let i = 0; i < attempts; i++) {
      try {
        receive(await call<AppState>("get_state"));
        return;
      } catch {
        await new Promise((r) => setTimeout(r, 150));
      }
    }
    if (!app) loadError = true;
  }

  async function save(s: Settings) {
    saveError = null;
    saving = true;
    try {
      receive(await call<AppState>("save_settings", { settings: s }));
    } catch (e) {
      saveError = String(e);
    } finally {
      saving = false;
    }
  }

  onMount(() => {
    void refresh(40);
    const offs = [
      on<AppState>("state-changed", receive),
      on<ProgressEvent>("model-progress", applyProgress),
      on<ProgressEvent>("text-model-progress", (p) => {
        const m = app?.text_models.find((x) => x.id === p.id);
        if (m) {
          m.state = "downloading";
          m.downloaded = p.downloaded;
        }
      }),
      on<HwTestStatus>("hwtest-progress", (s) => {
        if (app) app.hardware_test = s;
      }),
      on<null>("deleted", () => (deleted = true)),
    ];
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => {
      offs.forEach((p) => p.then((off) => off()));
      window.removeEventListener("focus", onFocus);
    };
  });

  function status(a: AppState): string {
    switch (a.engine.state) {
      case "ready": return `Ready — ${a.settings.hotkey.mode === "toggle" ? "press" : "hold"} ${a.hotkey_display}`;
      case "loading": return "Getting the model ready…";
      case "error": return "The model couldn't start";
      default: return "Download a model to begin";
    }
  }
</script>

{#if deleted}
  <div class="farewell"><Logo size={40} /><p>Aural is being removed. You can close this window.</p></div>
{:else if app}
  <Touchup preview={app.touchup} update={(p) => { if (app) app.touchup = latestTouchup(app.touchup, p); }} />
  <div class="shell">
    <aside>
      <div class="brand"><span class="mark"><Logo size={18} weight={6} /></span><span>Aural</span><span class="version">{app.version}</span></div>
      <div class="local-label">Voice, on your PC</div>
      <nav aria-label="Settings">
        {#each pages as p (p.id)}
          <button class="nav" aria-current={page === p.id ? "page" : undefined} onclick={() => (page = p.id)}>{p.label}</button>
        {/each}
      </nav>
      <div class="status" role="status">
        <span class="dot" class:ready={app.engine.state === "ready"}></span>
        <span>{status(app)}</span>
      </div>
      {#if !inApp}<div class="demo">Design preview — demo data</div>{/if}
    </aside>
    <main>
      <div class="save-status" role="status" aria-live="polite">{saving ? "Saving changes…" : saveError ? "Changes weren't saved" : "Settings save automatically"}</div>
      {#if app.notice}
        <div class="notice" role="alert">
          <span>{app.notice}</span>
          <button class="btn quiet" onclick={async () => receive(await call<AppState>("dismiss_notice"))}>Dismiss</button>
        </div>
      {/if}
      {#if saveError}
        <div class="notice" role="alert"><span>{saveError}</span><button class="btn quiet" onclick={() => (saveError = null)}>Dismiss</button></div>
      {/if}
      <fieldset class="page-content" disabled={saving} aria-busy={saving}>
      {#if page === "general"}<General {app} {save} />
      {:else if page === "microphone"}<Microphone {app} {save} />
      {:else if page === "models"}<Models {app} update={receive} />
      {:else if page === "writing"}<Writing {app} {save} update={receive} />
      {:else if page === "history"}<History {app} {save} update={receive} />
      {:else}<About {app} />{/if}
      </fieldset>
    </main>
  </div>
{:else}
  <div class="startup" role="status">
    <span class="mark"><Logo size={24} /></span>
    <h1>{loadError ? "Aural couldn't load settings" : "Opening Aural…"}</h1>
    <p>{loadError ? "Check that Aural is still running, then try again." : "Connecting to your local dictation engine."}</p>
    {#if loadError}<button class="btn primary" onclick={() => { loadError = false; void refresh(3); }}>Try again</button>{/if}
  </div>
{/if}

<style>
  .shell { display: flex; height: 100vh; overflow: hidden; }
  aside {
    display: flex;
    flex-direction: column;
    width: 208px;
    flex: none;
    padding: 20px 12px 16px;
    background: var(--side);
    border-right: 1px solid var(--line);
  }
  .brand { display: flex; align-items: center; gap: 10px; padding: 0 8px 8px; font: 600 16px var(--font-display); }
  .version { margin-left: auto; font: 11px var(--font); color: var(--ink-2); }
  .local-label { padding: 0 8px 24px; color: var(--ink-2); font-size: 12px; }
  .save-status { padding: 16px 36px 0; font-size: 11px; color: var(--ink-2); min-height: 32px; }
  .page-content { border: 0; margin: 0; padding: 0; min-width: 0; }
  .startup { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; height: 100vh; padding: 32px; text-align: center; }
  .startup .mark { width: 44px; height: 44px; }
  .startup h1 { font: 600 20px var(--font-display); margin: 0; }
  .startup p { color: var(--ink-2); margin: 0 0 8px; }
  .mark {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 8px;
    background: #0b0b0b;
    color: #fff;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.12);
  }
  nav { display: flex; flex-direction: column; gap: 2px; }
  .nav {
    height: 34px;
    padding: 0 10px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    text-align: left;
    color: var(--ink-2);
    font-weight: 600;
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  .nav:hover { background: var(--field); color: var(--ink); }
  .nav[aria-current="page"] { background: var(--bg); color: var(--ink); box-shadow: 0 0 0 1px var(--line); }
  .status {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 8px 0;
    color: var(--ink-2);
    font-size: 12px;
  }
  .dot { width: 8px; height: 8px; flex: none; border-radius: 50%; border: 1.5px solid var(--ink-3); }
  .dot.ready { background: var(--ink); border-color: var(--ink); }
  .demo { margin: 8px 8px 0; font-size: 11px; color: var(--ink-3); }
  main { position: relative; flex: 1; min-width: 0; overflow-y: auto; overscroll-behavior: contain; }
  .notice {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 16px 36px 0;
    padding: 10px 14px;
    border-radius: var(--radius);
    background: var(--invert-bg);
    color: var(--invert-ink);
  }
  .notice span { flex: 1; }
  .notice .btn.quiet { color: inherit; }
  .farewell { display: grid; place-items: center; align-content: center; gap: 12px; height: 100vh; color: var(--ink-2); }
</style>
