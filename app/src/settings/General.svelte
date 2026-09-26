<script lang="ts">
  import { onDestroy } from "svelte";
  import { call, on } from "../lib/api";
  import { ChordRecorder } from "../lib/keys";
  import type { AppState, HotkeyCheck, Settings } from "../lib/types";

  let { app, save }: { app: AppState; save: (s: Settings) => Promise<void> } = $props();

  let recording = $state(false);
  let proposal = $state<{ keys: string[]; check: HotkeyCheck } | null>(null);
  const recorder = new ChordRecorder();

  function onDown(e: KeyboardEvent) {
    e.preventDefault();
    if (e.code === "Escape" && !e.ctrlKey && !e.altKey && !e.metaKey) return stopRecording();
    if (!e.repeat) recorder.down(e);
  }

  async function onUp(e: KeyboardEvent) {
    e.preventDefault();
    const keys = recorder.up(e);
    if (!keys) return;
    try {
      proposal = { keys, check: await call<HotkeyCheck>("check_hotkey", { keys }) };
    } catch (err) {
      proposal = { keys, check: { display: keys.join(" + "), verdict: { verdict: "reject", message: String(err) } } };
    }
  }

  // Leaving the window (switching apps, closing it to the tray) ends recording, so the
  // real hotkey is never left paused.
  const onBlur = () => void stopRecording();
  let offHidden: (() => void) | null = null;

  async function startRecording() {
    proposal = null;
    recorder.reset();
    recording = true;
    await call("pause_hotkey", { paused: true });
    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);
    window.addEventListener("blur", onBlur);
    offHidden = await on("window-hidden", onBlur);
  }

  async function stopRecording() {
    if (!recording) return;
    window.removeEventListener("keydown", onDown, true);
    window.removeEventListener("keyup", onUp, true);
    window.removeEventListener("blur", onBlur);
    offHidden?.();
    offHidden = null;
    recording = false;
    proposal = null;
    await call("pause_hotkey", { paused: false });
  }

  async function useProposal() {
    if (!proposal || proposal.check.verdict.verdict === "reject") return;
    const keys = proposal.keys;
    await stopRecording();
    await save({ ...app.settings, hotkey: { ...app.settings.hotkey, keys } });
  }

  onDestroy(() => {
    if (recording) void stopRecording();
  });

  const setMode = (mode: Settings["hotkey"]["mode"]) =>
    save({ ...app.settings, hotkey: { ...app.settings.hotkey, mode } });
  const setAutostart = () =>
    save({ ...app.settings, startup: { launch_at_login: !app.settings.startup.launch_at_login } });
  const setPosition = (pill_position: Settings["ui"]["pill_position"]) =>
    save({ ...app.settings, ui: { pill_position } });

  let keys = $derived(app.hotkey_display.split(" + "));
</script>

<div class="page">
  <h1>General</h1>
  <p class="lede">Hold your hotkey anywhere in Windows, speak, and let go. Aural types what you said into the app you're using.</p>

  <section class="section">
    <h2>Dictation</h2>
    <div class="rows">
      <div class="row">
        <div class="text">
          <div class="title">Hotkey</div>
          {#if recording}
            <div class="desc">
              {#if proposal}
                {#if proposal.check.verdict.verdict === "reject"}{proposal.check.verdict.message}
                {:else if proposal.check.verdict.verdict === "warn"}{proposal.check.verdict.message}
                {:else}Looks good. Use it?{/if}
              {:else}Press the keys you want to use, then let go. Esc cancels.{/if}
            </div>
          {:else}
            <div class="desc">Works in every app. Win + H stays with Windows voice typing.</div>
          {/if}
        </div>
        <div class="control">
          {#if recording}
            <span class="keys" aria-live="polite">
              {#if proposal}
                {#each proposal.check.display.split(" + ") as k, i (i)}{#if i > 0}<span class="plus">+</span>{/if}<span class="key">{k}</span>{/each}
              {:else}<span class="key recording">Listening for keys…</span>{/if}
            </span>
            <button class="btn quiet" onclick={stopRecording}>Cancel</button>
            <button class="btn primary" onclick={useProposal} disabled={!proposal || proposal.check.verdict.verdict === "reject"}>Use</button>
          {:else}
            <span class="keys" aria-label="Current hotkey {app.hotkey_display}">
              {#each keys as k, i (i)}{#if i > 0}<span class="plus">+</span>{/if}<span class="key">{k}</span>{/each}
            </span>
            <button class="btn" onclick={startRecording}>Change</button>
          {/if}
        </div>
      </div>
      <div class="row">
        <div class="text">
          <div class="title">How the hotkey works</div>
          <div class="desc">
            {app.settings.hotkey.mode === "push_to_talk"
              ? "Hold to talk; Aural transcribes when you let go."
              : "Press once to start, press again to stop."}
          </div>
        </div>
        <div class="control seg" role="group" aria-label="Hotkey mode">
          <button aria-pressed={app.settings.hotkey.mode === "push_to_talk"} onclick={() => setMode("push_to_talk")}>Hold</button>
          <button aria-pressed={app.settings.hotkey.mode === "toggle"} onclick={() => setMode("toggle")}>Toggle</button>
        </div>
      </div>
    </div>
  </section>

  <section class="section">
    <h2>Startup</h2>
    <div class="rows">
      <div class="row">
        <div class="text">
          <div class="title">Start Aural with Windows</div>
          <div class="desc">Aural waits quietly in the notification area until you press the hotkey.</div>
        </div>
        <div class="control">
          <button
            class="switch"
            role="switch"
            aria-checked={app.settings.startup.launch_at_login}
            aria-label="Start Aural with Windows"
            onclick={setAutostart}
          ></button>
        </div>
      </div>
    </div>
  </section>

  <section class="section">
    <h2>Listening pill</h2>
    <div class="rows">
      <div class="row">
        <div class="text">
          <div class="title">Position</div>
          <div class="desc">Where the pill appears while you dictate, on the screen you are working on.</div>
        </div>
        <div class="control seg" role="group" aria-label="Pill position">
          <button aria-pressed={app.settings.ui.pill_position === "bottom"} onclick={() => setPosition("bottom")}>Bottom</button>
          <button aria-pressed={app.settings.ui.pill_position === "top"} onclick={() => setPosition("top")}>Top</button>
        </div>
      </div>
    </div>
  </section>
</div>

<style>
  .recording { color: var(--ink-2); font-weight: 500; }
</style>
