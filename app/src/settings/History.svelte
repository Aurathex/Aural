<script lang="ts">
  import { onMount } from "svelte";
  import { call, on } from "../lib/api";
  import { appLabel, KEEP_OPTIONS, keepText, spokenTime } from "../lib/copy";
  import type { AppState, HistoryEntry, Settings, StatsSummary } from "../lib/types";

  let {
    app,
    save,
    update,
  }: { app: AppState; save: (s: Settings) => Promise<void>; update: (s: AppState) => void } = $props();

  let query = $state("");
  let entries = $state<HistoryEntry[]>([]);
  let stats = $state<StatsSummary | null>(null);
  let editing = $state<number | null>(null);
  let draft = $state("");
  let confirmClear = $state(false);
  let error = $state<string | null>(null);

  async function load() {
    try {
      entries = await call<HistoryEntry[]>("history_search", { query, limit: 200 });
      stats = await call<StatsSummary>("stats_summary");
    } catch (e) {
      error = String(e);
    }
  }

  let timer: number | undefined;
  function search() {
    window.clearTimeout(timer);
    timer = window.setTimeout(load, 150);
  }

  onMount(() => {
    void load();
    // New dictations arrive while the window is open.
    const off = on<AppState>("state-changed", () => void load());
    return () => {
      window.clearTimeout(timer);
      void off.then((f) => f());
    };
  });

  const when = (at: number) =>
    new Date(at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });

  async function remove(id: number) {
    await call("history_delete", { id });
    await load();
  }

  async function clearAll() {
    await call("history_clear");
    confirmClear = false;
    await load();
  }

  function startEdit(e: HistoryEntry) {
    editing = e.id;
    draft = e.corrected ?? e.text;
  }

  async function saveEdit(id: number) {
    const text = draft.trim();
    if (text) update(await call<AppState>("history_correct", { id, text }));
    editing = null;
    await load();
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      /* the text stays visible to select */
    }
  }

  const setHistory = () =>
    save({ ...app.settings, history: { ...app.settings.history, enabled: !app.settings.history.enabled } });
  const setKeep = (keep_days: number) => save({ ...app.settings, history: { ...app.settings.history, keep_days } });
  const setStats = () =>
    save({ ...app.settings, history: { ...app.settings.history, stats: !app.settings.history.stats } });

  async function resetStats() {
    await call("stats_reset");
    await load();
  }

  let maxDay = $derived(stats ? Math.max(1, ...stats.recent.map(([, t]) => t.words)) : 1);
</script>

<div class="page">
  <h1>History</h1>
  <p class="lede">What you dictated, kept as text on this PC only, never audio. Search it, fix a mistake so Aural can learn, or delete it.</p>

  {#if error}<div class="callout" role="alert"><span class="text">{error}</span></div>{/if}

  {#if stats && app.settings.history.stats}
    <section class="section">
      <h2>Your dictation</h2>
      <div class="tiles">
        <div class="tile"><div class="big num">{stats.today.words.toLocaleString()}</div><div class="muted">words today</div></div>
        <div class="tile"><div class="big num">{stats.last_7_days.words.toLocaleString()}</div><div class="muted">words this week</div></div>
        <div class="tile"><div class="big num">{stats.all_time.dictations.toLocaleString()}</div><div class="muted">dictations in all</div></div>
        <div class="tile">
          <div class="big num">{stats.words_per_minute ? Math.round(stats.words_per_minute) : "–"}</div>
          <div class="muted">words per minute spoken</div>
        </div>
      </div>
      <div class="chart" role="img" aria-label="Words per day over the last two weeks">
        {#each stats.recent as [day, t] (day)}
          <div class="bar" title="{new Date(day * 86400000).toLocaleDateString()}: {t.words} words">
            <span style="height: {Math.round((t.words / maxDay) * 100)}%"></span>
          </div>
        {/each}
      </div>
      <p class="muted small">
        {spokenTime(stats.all_time.audio_ms)} spoken in all.
        {#if stats.apps.length}Most used in {stats.apps.slice(0, 3).map(([a]) => appLabel(a)).join(", ")}.{/if}
        {#if stats.all_time.dictations}Words showed while speaking in {stats.live} and were tidied in {stats.cleaned} of them.{/if}
      </p>
      <details class="tech">
        <summary>Technical details</summary>
        <dl>{#each stats.variants as [v, n] (v)}<dt>{v}</dt><dd>{n} dictations</dd>{/each}</dl>
      </details>
    </section>
  {/if}

  <section class="section">
    <h2>Dictations</h2>
    <input class="search" type="search" placeholder="Search your dictations" bind:value={query} oninput={search} aria-label="Search your dictations" />
    {#if !app.settings.history.enabled}
      <p class="muted">History is off, so new dictations aren't kept.</p>
    {/if}
    <div class="rows list">
      {#each entries as e (e.id)}
        <div class="row entry">
          <div class="text">
            <div class="meta muted small">{when(e.at)} · {appLabel(e.app)}{e.corrected ? " · corrected" : ""}</div>
            {#if editing === e.id}
              <textarea class="input edit" bind:value={draft} rows="3" aria-label="Your correction"></textarea>
              <div class="edit-actions">
                <span class="muted small">Aural compares your fix with what it wrote and suggests words for your dictionary.</span>
                <button class="btn quiet" onclick={() => (editing = null)}>Cancel</button>
                <button class="btn primary" onclick={() => saveEdit(e.id)}>Save</button>
              </div>
            {:else}
              <div class="said">{e.corrected ?? e.text}</div>
              {#if e.corrected}<div class="was muted small">Aural wrote: {e.text}</div>{/if}
            {/if}
          </div>
          {#if editing !== e.id}
            <div class="control">
              <button class="btn quiet" onclick={() => copy(e.corrected ?? e.text)}>Copy</button>
              <button class="btn quiet" onclick={() => startEdit(e)}>Fix</button>
              <button class="btn quiet" onclick={() => remove(e.id)} aria-label="Delete this dictation">Delete</button>
            </div>
          {/if}
        </div>
      {:else}
        <p class="muted empty">{query ? "Nothing matches your search." : "Nothing here yet. Your dictations will appear here."}</p>
      {/each}
    </div>
  </section>

  <section class="section">
    <h2>Keeping and deleting</h2>
    <div class="rows">
      <div class="row">
        <div class="text">
          <div class="title">Keep a history of my dictations</div>
          <div class="desc">Text only, on this PC. Apps can be left out on the Writing page.</div>
        </div>
        <div class="control">
          <button class="switch" role="switch" aria-checked={app.settings.history.enabled} aria-label="Keep a history of my dictations" onclick={setHistory}></button>
        </div>
      </div>
      <div class="row">
        <div class="text">
          <div class="title">Keep dictations for</div>
          <div class="desc">Older dictations are deleted automatically.</div>
        </div>
        <div class="control">
          <select class="field keep" aria-label="Keep dictations for" value={app.settings.history.keep_days} onchange={(ev) => setKeep(Number(ev.currentTarget.value))}>
            {#each KEEP_OPTIONS as d (d)}<option value={d}>{keepText(d)}</option>{/each}
          </select>
        </div>
      </div>
      <div class="row">
        <div class="text">
          <div class="title">Delete all dictations</div>
          <div class="desc">{app.history_count} kept now. This can't be undone.</div>
        </div>
        <div class="control">
          {#if confirmClear}
            <button class="btn quiet" onclick={() => (confirmClear = false)}>Cancel</button>
            <button class="btn danger" onclick={clearAll}>Delete all</button>
          {:else}
            <button class="btn" onclick={() => (confirmClear = true)} disabled={app.history_count === 0}>Delete all…</button>
          {/if}
        </div>
      </div>
      <div class="row">
        <div class="text">
          <div class="title">Count my dictation</div>
          <div class="desc">Words, time and apps, as numbers only, for the summary above.</div>
        </div>
        <div class="control">
          <button class="btn quiet" onclick={resetStats}>Reset</button>
          <button class="switch" role="switch" aria-checked={app.settings.history.stats} aria-label="Count my dictation" onclick={setStats}></button>
        </div>
      </div>
    </div>
  </section>
</div>

<style>
  .tiles { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; margin-bottom: 12px; }
  .tile { padding: 12px 14px; border: 1px solid var(--line); border-radius: var(--radius); }
  .big { font: 600 22px/1.2 var(--font-display); }
  .chart { display: flex; align-items: flex-end; gap: 4px; height: 56px; margin: 4px 0 8px; }
  .bar { flex: 1; height: 100%; display: flex; align-items: flex-end; }
  .bar span { display: block; width: 100%; min-height: 2px; border-radius: 2px 2px 0 0; background: var(--ink); opacity: 0.85; }
  .small { font-size: 12px; }
  select.field.keep { min-width: 180px; }
  .search {
    width: 100%;
    height: 32px;
    padding: 0 12px;
    margin-bottom: 8px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--ink);
    user-select: text;
  }
  .entry { align-items: flex-start; }
  .said { user-select: text; overflow-wrap: anywhere; }
  .was { margin-top: 2px; user-select: text; }
  .meta { margin-bottom: 2px; }
  .empty { padding: 12px 0; }
  .input.edit {
    width: 100%;
    padding: 6px 10px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--ink);
    font: inherit;
    resize: vertical;
    user-select: text;
  }
  .edit-actions { display: flex; align-items: center; gap: 8px; margin-top: 6px; }
  .edit-actions span { flex: 1; }
  details.tech { font-size: 12px; color: var(--ink-3); }
  details.tech summary { cursor: pointer; width: fit-content; }
  dl { display: grid; grid-template-columns: max-content 1fr; gap: 2px 12px; margin: 6px 0 0; }
  dt { font-weight: 600; }
  dd { margin: 0; }
  @media (max-width: 760px) {
    .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }
</style>
