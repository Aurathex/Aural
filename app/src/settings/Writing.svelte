<script lang="ts">
  import { call } from "../lib/api";
  import { appLabel, cleanupText } from "../lib/copy";
  import { formatBytes } from "../lib/format";
  import type { AppProfile, AppState, CleanupMode, DictionaryEntry, Settings } from "../lib/types";

  let {
    app,
    save,
    update,
  }: { app: AppState; save: (s: Settings) => Promise<void>; update: (s: AppState) => void } = $props();

  const modes: { id: CleanupMode; label: string }[] = [
    { id: "off", label: "Off" },
    { id: "light", label: "Light" },
    { id: "ai", label: "AI" },
  ];

  let error = $state<string | null>(null);
  async function run(cmd: string, args?: Record<string, unknown>) {
    error = null;
    try {
      update(await call<AppState>(cmd, args));
    } catch (e) {
      error = String(e);
    }
  }

  const setCleanup = (cleanup: CleanupMode) =>
    save({ ...app.settings, text: { ...app.settings.text, cleanup } });
  const setDictionaryOn = () =>
    save({ ...app.settings, text: { ...app.settings.text, dictionary: !app.settings.text.dictionary } });

  // Writing helper (AI cleanup model).
  let helper = $derived(app.text_models[0] ?? null);
  let helperReady = $derived(app.text_engine.state === "ready");

  // Dictionary.
  let newWrite = $state("");
  let newHeard = $state("");
  function addEntry() {
    const write = newWrite.trim();
    if (!write) return;
    const heard = newHeard.split(",").map((h) => h.trim()).filter(Boolean);
    const entries: DictionaryEntry[] = app.words.entries.filter((e) => e.write !== write);
    entries.push({ write, heard });
    newWrite = "";
    newHeard = "";
    void run("set_dictionary", { entries });
  }
  const removeEntry = (i: number) =>
    run("set_dictionary", { entries: app.words.entries.filter((_, k) => k !== i) });

  // Per-app settings.
  let newApp = $state("");
  let candidates = $derived(
    app.recent_apps.filter((a) => !app.settings.apps.some((p) => appLabel(p.app).toLowerCase() === appLabel(a).toLowerCase())),
  );
  function addApp(name: string) {
    const exe = name.trim();
    if (!exe) return;
    const file = /\.exe$/i.test(exe) ? exe : `${exe}.exe`;
    if (app.settings.apps.some((p) => appLabel(p.app).toLowerCase() === appLabel(file).toLowerCase())) return;
    const profile: AppProfile = { app: file.toLowerCase(), cleanup: null, dictionary: null, live: null, history: null };
    newApp = "";
    void save({ ...app.settings, apps: [...app.settings.apps, profile] });
  }
  function setApp(i: number, patch: Partial<AppProfile>) {
    const apps = app.settings.apps.map((p, k) => (k === i ? { ...p, ...patch } : p));
    void save({ ...app.settings, apps });
  }
  const removeApp = (i: number) =>
    save({ ...app.settings, apps: app.settings.apps.filter((_, k) => k !== i) });
  // Three-way choice for a yes/no setting: follow the general setting, on, or off.
  const tri = (v: boolean | null) => (v === null ? "default" : v ? "on" : "off");
  const fromTri = (v: string) => (v === "default" ? null : v === "on");
</script>

<div class="page">
  <h1>Writing</h1>
  <p class="lede">How your words are tidied before they're typed, the words Aural should always spell your way, and settings for particular apps. All of it stays on this PC.</p>

  {#if error}<div class="callout" role="alert"><span class="text">{error}</span></div>{/if}

  <section class="section">
    <h2>Tidy up</h2>
    <div class="rows">
      <div class="row">
        <div class="text">
          <div class="title">Tidy up what I say</div>
          <div class="desc">{cleanupText(app.settings.text.cleanup)}</div>
        </div>
        <div class="control seg" role="group" aria-label="Tidy up">
          {#each modes as m (m.id)}
            <button aria-pressed={app.settings.text.cleanup === m.id} onclick={() => setCleanup(m.id)}>{m.label}</button>
          {/each}
        </div>
      </div>
      {#if helper}
        <div class="row">
          <div class="text">
            <div class="title">{helper.name}</div>
            <div class="desc">
              {#if helper.state === "installed"}
                {#if app.text_engine.state === "loading"}Getting ready…
                {:else if helperReady}Ready. Used when tidy-up is set to AI.
                {:else if app.text_engine.state === "error"}It couldn't start on this PC, so the light tidy-up is used.
                {:else}Downloaded. It starts when tidy-up is set to AI.{/if}
              {:else if helper.state === "downloading"}
                Downloading… {formatBytes(helper.downloaded)} of {formatBytes(helper.size_bytes)}
              {:else}
                Needed for AI tidy-up. {helper.description} Downloads {formatBytes(helper.size_bytes)} once, only if you choose to.
              {/if}
            </div>
            {#if helper.state === "downloading"}
              <div class="progress" aria-hidden="true"><span style="transform: scaleX({helper.downloaded / helper.size_bytes})"></span></div>
            {/if}
            <details class="tech">
              <summary>Technical details</summary>
              <p>{helper.attribution} License: {helper.license_id}. Needs about {formatBytes(helper.min_ram_mb * 1e6)} of memory while in use.</p>
              {#if app.text_engine.state === "error"}<p>{app.text_engine.message}</p>{/if}
            </details>
          </div>
          <div class="control">
            {#if helper.state === "available"}
              <button class="btn primary" onclick={() => run("download_text_model", { id: helper.id })}>Download {formatBytes(helper.size_bytes)}</button>
            {:else if helper.state === "downloading"}
              <button class="btn" onclick={() => call("cancel_download", { id: helper.id })}>Cancel</button>
            {:else}
              <button class="btn" onclick={() => run("remove_text_model", { id: helper.id })}>Remove</button>
            {/if}
          </div>
        </div>
        {#if app.settings.text.cleanup === "ai" && helper.state !== "installed"}
          <div class="row note"><div class="text desc">Until the writing helper is downloaded, the light tidy-up is used.</div></div>
        {/if}
      {/if}
    </div>
  </section>

  <section class="section">
    <h2>Dictionary</h2>
    <div class="rows">
      <div class="row">
        <div class="text">
          <div class="title">Use my dictionary</div>
          <div class="desc">Names and words written your way, even when the speech model hears them differently.</div>
        </div>
        <div class="control">
          <button class="switch" role="switch" aria-checked={app.settings.text.dictionary} aria-label="Use my dictionary" onclick={setDictionaryOn}></button>
        </div>
      </div>
      {#each app.words.entries as e, i (e.write)}
        <div class="row">
          <div class="text">
            <div class="title word">{e.write}</div>
            <div class="desc">{e.heard.length ? `Also when I say: ${e.heard.join(", ")}` : "Always spelled this way"}</div>
          </div>
          <div class="control"><button class="btn quiet" onclick={() => removeEntry(i)} aria-label="Remove {e.write}">Remove</button></div>
        </div>
      {/each}
      <form class="row add" onsubmit={(ev) => { ev.preventDefault(); addEntry(); }}>
        <label class="field-group">
          <span>Write it as</span>
          <input class="input" bind:value={newWrite} placeholder="e.g. Aurathex" maxlength="80" />
        </label>
        <label class="field-group grow">
          <span>When I say (optional, separate with commas)</span>
          <input class="input" bind:value={newHeard} placeholder="e.g. aura thex, or a thex" maxlength="200" />
        </label>
        <div class="control"><button class="btn" type="submit" disabled={!newWrite.trim()}>Add</button></div>
      </form>
    </div>
  </section>

  {#if app.words.learned.suggestions.length}
    <section class="section">
      <h2>Suggestions from your corrections</h2>
      <div class="rows">
        {#each app.words.learned.suggestions as s, i (s.heard + s.write)}
          <div class="row">
            <div class="text">
              <div class="title">“{s.heard}” → “{s.write}”</div>
              <div class="desc">You made this change {s.count === 1 ? "once" : `${s.count} times`} in History. Nothing changes unless you add it.</div>
            </div>
            <div class="control">
              <button class="btn quiet" onclick={() => run("dismiss_suggestion", { index: i })}>Ignore</button>
              <button class="btn" onclick={() => run("accept_suggestion", { index: i })}>Add to dictionary</button>
            </div>
          </div>
        {/each}
      </div>
    </section>
  {/if}
  <div class="learn-reset">
    <button class="btn quiet" onclick={() => run("reset_learning")}>Forget what Aural learned from my corrections</button>
  </div>

  <section class="section">
    <h2>Apps</h2>
    <p class="muted intro">Change the settings for one app. Anything left on “Default” follows the settings above; apps not listed use them too.</p>
    <div class="rows">
      {#each app.settings.apps as p, i (p.app)}
        <div class="row app-row">
          <div class="text"><div class="title">{appLabel(p.app)}</div></div>
          <label class="mini">Tidy up
            <select class="field small" value={p.cleanup ?? "default"} onchange={(ev) => setApp(i, { cleanup: ev.currentTarget.value === "default" ? null : (ev.currentTarget.value as CleanupMode) })}>
              <option value="default">Default</option><option value="off">Off</option><option value="light">Light</option><option value="ai">AI</option>
            </select>
          </label>
          <label class="mini">Words while speaking
            <select class="field small" value={tri(p.live)} onchange={(ev) => setApp(i, { live: fromTri(ev.currentTarget.value) })}>
              <option value="default">Default</option><option value="on">Show</option><option value="off">Don't show</option>
            </select>
          </label>
          <label class="mini">History
            <select class="field small" value={tri(p.history)} onchange={(ev) => setApp(i, { history: fromTri(ev.currentTarget.value) })}>
              <option value="default">Default</option><option value="on">Keep</option><option value="off">Don't keep</option>
            </select>
          </label>
          <label class="mini">Dictionary
            <select class="field small" value={tri(p.dictionary)} onchange={(ev) => setApp(i, { dictionary: fromTri(ev.currentTarget.value) })}>
              <option value="default">Default</option><option value="on">Use</option><option value="off">Don't use</option>
            </select>
          </label>
          <div class="control"><button class="btn quiet" onclick={() => removeApp(i)} aria-label="Remove settings for {appLabel(p.app)}">Remove</button></div>
        </div>
      {/each}
      <form class="row add" onsubmit={(ev) => { ev.preventDefault(); addApp(newApp); }}>
        <label class="field-group grow">
          <span>Add an app</span>
          <input class="input" list="recent-apps" bind:value={newApp} placeholder="Program name, e.g. slack or code" maxlength="80" />
          <datalist id="recent-apps">{#each candidates as a (a)}<option value={appLabel(a)}></option>{/each}</datalist>
        </label>
        <div class="control"><button class="btn" type="submit" disabled={!newApp.trim()}>Add</button></div>
      </form>
      {#if candidates.length}
        <div class="row chips-row">
          <span class="muted">Apps you dictated into:</span>
          {#each candidates.slice(0, 8) as a (a)}<button class="chip-btn" onclick={() => addApp(a)}>{appLabel(a)}</button>{/each}
        </div>
      {/if}
    </div>
  </section>
</div>

<style>
  .word { user-select: text; }
  .note .desc { color: var(--ink-2); }
  .add { align-items: flex-end; flex-wrap: wrap; gap: 12px 16px; }
  .field-group { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--ink-2); min-width: 0; }
  .field-group.grow { flex: 1; }
  .input {
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--ink);
    min-width: 0;
    user-select: text;
  }
  .learn-reset { margin: -16px 0 28px; }
  .intro { margin: 0 0 8px; }
  .app-row { flex-wrap: wrap; gap: 8px 16px; }
  .app-row .text { flex: 1 0 120px; }
  .mini { display: flex; flex-direction: column; gap: 2px; font-size: 11px; color: var(--ink-3); }
  select.field.small { min-width: 0; width: 120px; height: 28px; }
  .chips-row { flex-wrap: wrap; gap: 6px; min-height: 0; }
  .chip-btn {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--line);
    border-radius: 13px;
    background: var(--bg);
    font-weight: 600;
    font-size: 12px;
    cursor: pointer;
  }
  .chip-btn:hover { background: var(--field); }
  .progress { position: relative; height: 4px; margin-top: 8px; border-radius: 2px; background: var(--line); overflow: hidden; }
  .progress span { position: absolute; inset: 0; background: var(--ink); transform-origin: left; transition: transform 200ms linear; }
  details.tech { margin-top: 6px; font-size: 12px; color: var(--ink-3); }
  details.tech summary { cursor: pointer; width: fit-content; }
  details.tech p { margin: 4px 0 0; user-select: text; }
</style>
