# Manual test checklist

Automated tests cover the logic, the Windows APIs they can drive (keyboard hook with
synthetic keys, pasting into a test window, registry, microphone capture, the real
installer and uninstaller) and the worker processes. What's left needs a person at the
keyboard. Run this on a real Windows 10 or 11 PC before each release; record results in
the release notes.

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
