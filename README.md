# Aural

Local voice dictation for Windows. Hold a hotkey, speak, let go — your words are
transcribed **on your own PC** and typed into whatever app you're using.

- No account, no telemetry, no cloud audio. The network is used only when you
  download a speech model.
- Hold **Ctrl + Win** (changeable) to dictate. A small black pill shows it's listening.
- Speech models are downloaded from inside the app, never bundled.
- Made for English. (Parakeet v3, one of the optional models, also understands 24 other
  European languages.)
- Words appear above the pill as you speak, and the final text is typed when you finish.
- Optional tidy-up on your PC (light rules, or a writing helper model you choose to
  download), a personal dictionary, settings per app, and a searchable history with
  statistics that never leave your PC.

## Download Aural

The **only** official download is the Releases page of the official repository:
<https://github.com/Aurathex/Aural/releases>

1. Open the latest release (currently
   [Aural 0.3.0](https://github.com/Aurathex/Aural/releases/tag/v0.3.0)).
2. Under **Assets**, download **`Aural_0.3.0_x64-setup.exe`** (the Windows installer,
   64-bit; for a newer release the version number in the name changes).

Aural runs on 64-bit Windows 10 (version 1809 or later) and Windows 11. It isn't in the
Microsoft Store or WinGet. Copies from anywhere else (mirrors, download sites, "cracked"
or "pro" versions, re-uploads) are not official, and redistributing Aural is not
permitted by its license.

### The installer is not code-signed

Aural's first releases are **not code-signed**. What that means for you:

- **Windows SmartScreen will probably warn you.** For a new, unsigned download, Windows
  usually shows **"Windows protected your PC"** and names the publisher as unknown. The
  warning is Windows saying it can't vouch for the file. It does not mean the file is
  known to be harmful, and it does not mean the file is safe either.
- **If you are not comfortable continuing, choose Don't run and stop.** That's a
  reasonable choice. Nothing about Aural requires you to take a risk you aren't happy
  with.
- **If you have checked where the file came from** (the steps below) and decide to
  continue, select **More info**, check that the file name is the one you downloaded,
  then select **Run anyway**. Don't click through security warnings for files whose
  origin you haven't checked.
- **Smart App Control** (Windows 11) blocks unsigned apps outright, with no option to
  continue. If it is on, Aural can't be installed for now. We don't recommend turning
  Smart App Control off just for Aural.
- **Antivirus software may hold Aural for analysis.** In testing, one antivirus product
  sandboxed the unsigned uninstaller, which then appeared to hang. Let your antivirus
  finish its check, or use its own tools to review the file. Don't turn protection off
  unless you have verified the download.

### Check the download before you run it

1. **Source.** The address bar showed `https://github.com/Aurathex/Aural/releases/...`
   when you downloaded it.
2. **Hash.** Compare the installer's SHA-256 with the line for it in `SHA256SUMS.txt`
   on the same release page:

   ```powershell
   Get-FileHash .\Aural_0.3.0_x64-setup.exe -Algorithm SHA256
   ```

3. **Build provenance.** Each release installer is built by the repository's public
   release workflow, which publishes a signed build attestation. With the
   [GitHub CLI](https://cli.github.com/) you can check that the file you have is exactly
   what that workflow built, and from which commit:

   ```powershell
   gh attestation verify .\Aural_0.3.0_x64-setup.exe --repo Aurathex/Aural
   ```

### Inspect it yourself, or ask an AI coding agent to

The complete source is public. Before installing you can read it yourself, or point an
AI coding agent (for example Claude Code) at the repository and ask it to review it.
Worth looking at:

| What | Where |
|---|---|
| What the installer and uninstaller do | `app/src-tauri/tauri.conf.json`, `app/src-tauri/windows/hooks.nsh` |
| How releases are built and published | `.github/workflows/release.yml`, `docs/releasing.md` |
| Every dependency, pinned | `Cargo.lock`, `app/package-lock.json`, `rust-toolchain.toml`; policy in `deny.toml` |
| Where speech models come from, with their SHA-256 | `manifests/catalog.v2.json` (Hugging Face URLs pinned to exact commits) |
| What Aural sends over the network (only model downloads) | `PRIVACY.md`, `crates/aural-models/src/download.rs` |
| Keyboard hook, clipboard and text insertion | `crates/aural-platform/src/` |
| Third-party licenses | `THIRD_PARTY_NOTICES.md` |

A prompt you could use: *"Review github.com/Aurathex/Aural at tag v0.3.0. Check what the
installer and uninstaller change on my PC, every network request the app can make,
where speech models are downloaded from and how they are verified, how the keyboard
hook and clipboard are used, and whether the release workflow could ship anything other
than what is in the repository."* An AI review is a useful second look. It is not a
guarantee.

## Install

1. Double-click **`Aural_0.3.0_x64-setup.exe`**. If Windows SmartScreen warns you, see
   [The installer is not code-signed](#the-installer-is-not-code-signed) above.
2. Follow the installer. It installs Aural **for your Windows user only** and doesn't
   need administrator rights. The default folder is `%LOCALAPPDATA%\Aural`.
3. Aural's windows use Microsoft Edge WebView2, which is part of Windows 11 and most
   Windows 10 PCs. If it's missing, the installer downloads and installs it, so keep the
   PC online.
4. On the last page, leave **Run Aural** ticked to start it now. Tick **Create desktop
   shortcut** if you want one. Aural is always added to the Start menu.

To start Aural later, open the Start menu and choose **Aural**. It runs in the
notification area (bottom-right, next to the clock): click its icon, or right-click it
and choose **Open Aural**, to see its window; **Quit Aural** closes it completely.

## First run

1. Aural opens on **Models** and offers to check your PC: it tries two small test models
   (about 75 MB, only if you agree) and then shows which models suit this PC, in plain
   words. Download the one marked **Recommended** (often Parakeet TDT 0.6B v2, 661 MB).
   Downloads are checked against a fixed SHA-256 before use.
2. Click into any app, hold **Ctrl + Win**, speak, let go. The text appears where your
   cursor is.
3. Closing the window keeps Aural in the notification area. Turn on **Start Aural with
   Windows** under General if you want it there after every sign-in.

If something goes wrong the pill says so briefly — for example "Mic blocked" (Windows
privacy settings), "No model", or "Copied — Ctrl+V" when Aural couldn't type into the
window (admin apps, or you switched windows) and left the text on your clipboard.

## Update

Aural doesn't check for updates by itself. New versions are published on the
[Releases page](https://github.com/Aurathex/Aural/releases) (on GitHub you can choose
**Watch → Custom → Releases** to be notified).

1. Download the new `Aural_<version>_x64-setup.exe` from the Releases page and, ideally,
   check it as described above.
2. Run it. If Aural is still running, the installer offers to close it (or quit it first:
   right-click the notification-area icon → **Quit Aural**).
3. When the installer says an older version of Aural is installed, keep the recommended
   **Uninstall before installing** and continue.

Your settings, history and downloaded models are kept, and "Start Aural with Windows"
stays as it was.

## Uninstall

- **Windows 11:** Settings → Apps → Installed apps → **Aural** → **⋯** → Uninstall.
- **Windows 10:** Settings → Apps → Apps & features → **Aural** → Uninstall.

The uninstaller removes the program and the "start with Windows" entry. Its **Delete the
application data** box decides what happens to your data: leave it unticked to keep
your settings, history and downloaded models (for example if you'll reinstall later), or
tick it to remove them too.

To remove everything from inside the app instead: open Aural → **About** → **Delete
Aural** (type `YES, DELETE` to confirm). It removes the app, every downloaded model,
your settings, history and dictionary, and the startup entry.

## Documentation

- [Privacy](PRIVACY.md) · [Security](SECURITY.md) · [Licensing](docs/LICENSING.md)
- [Building on Windows](docs/building-windows.md) · [Releasing](docs/releasing.md)
- [Design spec](docs/superpowers/specs/2026-09-25-aural-design.md) ·
  [Benchmark tool](tools/bench/README.md)

## License

Aural is **source-available**, not open source. The source is public, and you may use
Aural for free for personal and other **noncommercial** purposes under the
[PolyForm Strict License 1.0.0](LICENSE.md). The license does not allow modifying or
redistributing Aural. Commercial use needs a separate license from Aurathex.
See [docs/LICENSING.md](docs/LICENSING.md) for the reasoning. Speech models and
third-party components keep their own licenses ([THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)).
