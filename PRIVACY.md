# Privacy

Aural is built so your voice never leaves your PC.

## What Aural does

- **Transcribes locally.** Speech is recognised by a model running on your computer, in
  a separate Aural process.
- **Holds audio in memory only.** The microphone is opened when you press the hotkey (or
  run the microphone test) and closed when you let go. The recording is transcribed and
  discarded; it is never written to disk.
- **Types through the clipboard, privately.** Dictated text is placed on the clipboard
  marked so that Windows clipboard history (Win + V), cloud clipboard sync and clipboard
  monitors skip it; your previous clipboard text is put back afterwards. If Aural can't
  type into the window, the text is left on the clipboard for you to paste.
- **Shows words while you speak** (if on): the recording is streamed to the speech model
  on your PC as it happens; nothing else sees it, and it is still never written to disk.
- **Remembers the last transcript** in memory, so the tray menu can copy it again. It is
  gone when Aural quits.
- **Keeps a history of what you dictated** (text only, never audio) so you can search
  it, fix mistakes and learn words from your fixes. It is on by default, kept for 30 days,
  and can be turned off, shortened, or deleted (one entry or all) on the History page.
  Particular apps can be left out (for example a password manager) on the Writing page.
- **Counts your dictation** (numbers only: dictations, words, time spoken, which model
  and which app) for the summary on the History page. It can be turned off or reset.
- **Tidies text on your PC.** Light tidy-up and the dictionary are plain rules. AI tidy-up
  uses a writing helper model that runs on your PC in its own Aural process.

## What Aural does not do

- No account, sign-in or licence server.
- No telemetry or crash reporting. The statistics above never leave your PC.
- No cloud AI: tidy-up, the dictionary and learning all run locally.
- No cloud speech recognition and no fallback to one.

## When Aural uses the network

Only when **you** click **Download** on the Models page or for the writing helper on the
Writing page (about 793 MB), or agree to the PC check's two small test models (about
75 MB). Aural then fetches the model
files from Hugging Face (`huggingface.co` and its download CDN) over HTTPS, at fixed,
versioned addresses, and checks each file's SHA-256 before using it. The model
catalogue is built into the app, so Aural never "checks in" on its own. There is no
automatic update check.

The settings window is a web view with no network access of its own (its content
security policy only allows talking to Aural itself).

## Files Aural keeps

| Where | What |
|---|---|
| `%LOCALAPPDATA%\Aural` | the program |
| `%LOCALAPPDATA%\com.aurathex.aural` | downloaded models (`models`, and `text-models` for the writing helper), your dictation history (`history.jsonl`, text only) and statistics (`stats.json`, numbers only), the PC check's results (`hardware.json`: speed and accuracy of each model on this PC, and a description of the processor, memory and graphics cards) and the settings window's web view cache |
| `%APPDATA%\com.aurathex.aural\settings.json` | your settings (hotkey, microphone, model, startup, tidy-up, per-app settings, history) |
| `%APPDATA%\com.aurathex.aural\dictionary.json` | your dictionary and the suggestions learned from your fixes |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Aural` | only if you turn on "Start Aural with Windows" |

**Delete Aural** (Settings → About) removes all of these.
