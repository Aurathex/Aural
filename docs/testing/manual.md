# Manual test checklist

Automated tests cover the logic, the Windows APIs they can drive (keyboard hook with
synthetic keys, pasting into a test window, registry, microphone capture, the real
installer and uninstaller) and the worker processes. What's left needs a person at the
keyboard. Run this on a real Windows 10 or 11 PC before each release; record results in
the release notes.

Run installer checks from an ordinary terminal. A terminal inside a packaged (MSIX)
app, such as a desktop app's built-in shell, has its AppData and HKCU writes redirected
into that app's container: the installer then "succeeds" in a private copy that Windows
itself never sees.

Before starting, with the screen unlocked, run the automated desktop tests and the
installer upgrade check:

```powershell
cargo test -p aural-platform -- --ignored --test-threads=1
./scripts/test-upgrade-autostart.ps1 -Installer target/release/bundle/nsis/Aural_<version>_x64-setup.exe
./scripts/test-upgrade-autostart.ps1 -Installer target/release/bundle/nsis/Aural_<version>_x64-setup.exe -Off
```

## Hotkey and pill

- [ ] Hold **Ctrl + Win**, speak a sentence, let go: text appears at the cursor.
- [ ] Releasing Ctrl + Win does **not** open the Start menu.
- [ ] The pill appears above the taskbar on the monitor you're working on, shows moving
      bars while you speak, a sweep while transcribing, a line on success, then fades.
- [ ] The app you were typing in **keeps focus** the whole time (the caret keeps
      blinking; typing right after dictation goes to the same place).
- [ ] A quick tap of the hotkey (under a quarter second) does nothing.
- [ ] Holding the hotkey and saying nothing inserts nothing.
- [ ] Ctrl + Win + → (switch desktop) still works and does not start dictation.
- [ ] Esc while holding the hotkey cancels.
- [ ] Toggle mode: press once to start, once to stop.
- [ ] Change the hotkey to Right Ctrl in Settings; the new one works immediately and
      the old one doesn't.
- [ ] Close the Settings window with its **X** button: Aural stays in the notification
      area and dictation with the hotkey still works in another app.
- [ ] Click **Change** for the hotkey, then close the Settings window (or switch to
      another app) without finishing: dictation with the current hotkey still works.

## Where text goes

For each: dictate "Testing Aural, one two three." and check the text, punctuation and
that nothing else happened (no message sent, no command run).

- [ ] Notepad
- [ ] Word / Outlook (text takes the document's formatting)
- [ ] Chrome and Edge: a textarea, a Google Docs document, a search box
- [ ] Firefox
- [ ] VS Code editor
- [ ] VS Code integrated terminal and Windows Terminal (PowerShell): text arrives on one
      line and is **not executed**
- [ ] Slack or Teams or Discord message box: the message is **not sent**
- [ ] Notepad run as administrator: pill says "Copied — Ctrl+V" and Ctrl+V pastes it
- [ ] Switch windows between releasing the hotkey and the text arriving: pill says
      "Copied — Ctrl+V"; nothing is typed into the new window
- [ ] Copy an image first, then dictate: the image is still on the clipboard afterwards
- [ ] Copy some text first, then dictate: that text is still on the clipboard afterwards,
      and dictated text does not appear in Win + V history

## 0.3.1 terminal and settings regression checks

The earlier background-reader failures were resolved with target-reader identity,
conservative unconfirmed delivery, cleanup contention handling, and transaction-owner
scoping. The full platform suite now passes with SignalRGB running, without exclusions.
Native selection replacement and actual Windows Terminal input-content fixtures pass.
A clipboard read alone still does not prove insertion: unknown readers produce
"Check target input" and preserve recovery rather than automatic retry.
The checklist below distinguishes manual microphone/application checks from the
completed automated/native fixtures; see docs/release/v0.3.1-readiness.md.

- [ ] Windows Terminal / PowerShell: dictate into an empty prompt, check it is not
      submitted, and verify that quoted repeated spaces and paths survive delivery.
- [ ] Claude Code in Windows Terminal: dictate into the prompt in normal input mode;
      the prompt must remain editable and must not be submitted automatically.
- [ ] VS Code and Cursor integrated terminals: in Writing → Apps add the host app
      and select Terminal paste; repeat the Claude Code check. Switch delivery back
      to Automatic and verify normal editor input. The override applies to the host,
      not exclusively its terminal. Custom bindings may require another shortcut.
- [ ] Change focus while transcription is finishing: neither the old nor new app
      receives automatic text; recover the result from the clipboard/tray.
- [ ] An elevated terminal receives clipboard-only recovery from non-elevated Aural.
- [ ] Toggle mode uses “press” in status/help; push-to-talk uses “hold”.
- [ ] A disconnected settings backend shows a retry action instead of a blank window.
- [ ] Check General, Models, Writing, History and About at 720×520, 820×580 and
      1280×800, in light/dark mode and with increased Windows text scaling.
- [ ] Saving disables settings controls until completion and reports a save failure.

## Microphone

- [ ] Choose a different microphone in Settings; dictation uses it.
- [ ] Unplug that microphone; dictation falls back to the Windows default.
- [ ] Turn off "Let desktop apps access your microphone": Settings shows the warning,
      the pill says "Mic blocked".

## Models, startup, removal

- [ ] Fresh install opens on Models; downloading the recommended model makes Aural ready.
- [ ] Turn on "Start Aural with Windows", sign out and in: Aural is in the notification
      area without opening a window.
- [ ] Delete Aural removes the app, the Start menu entry and all data.


### Selected-text document identity limit

AI touch-up displays the original window caption before Apply. Delivery checks the
original process/window, focused control, window caption, accessibility identity
when available, and exact selected text. Generic applications can reuse all these
identifiers across documents. If a different document has the same caption,
focused-control identity and selected text, Aural cannot distinguish it from the
original document. These checks reduce accidental replacement; they do not prove
absolute document identity. Keep the original document open and verify the target
caption and revision before Apply. Do not switch documents while a preview is open.

Manual acceptance: hide a review preview, press Ctrl + Alt + T again, and verify
that the same preview reopens. Verify the original window caption in that preview.
