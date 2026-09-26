# Security

## Reporting a vulnerability

Please report security problems privately through GitHub's **Report a vulnerability**
button on this repository's Security tab rather than in a public issue. You'll get a
reply within a week. Please include the Aural version, Windows version and steps to
reproduce.

## Design notes relevant to security

- **Keyboard hook.** Aural installs a low-level keyboard hook to detect its hotkey. The
  hook only compares key codes with your chosen hotkey; it does not record or store
  keystrokes, and it ignores input that other programs inject.
- **Simulated input.** To insert text Aural sends a paste shortcut (or Unicode key
  events) to the window that had focus when you released the hotkey. If focus moved,
  or the target runs as administrator, Aural does not type and leaves the text on the
  clipboard instead.
- **Downloads.** Model files come from fixed, commit-pinned Hugging Face URLs over HTTPS
  and are rejected unless their SHA-256 matches the value compiled into Aural. File
  names in the catalogue are validated so a download can't write outside its folder.
- **Speech workers.** Models run in separate processes that talk to Aural over a framed
  pipe protocol; a crashed or hung worker is restarted rather than taking Aural down.
- **Web view.** The settings window cannot reach the network or the file system
  directly; it can only call Aural's own commands, which validate their input (for
  example Delete Aural re-checks the confirmation phrase and refuses to remove any
  folder other than Aural's own).
- **Unsigned builds.** Releases are not code-signed yet; verify downloads with the
  published `SHA256SUMS.txt`.
