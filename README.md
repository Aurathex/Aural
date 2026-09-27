# Aural

Local voice dictation for Windows. Hold a hotkey, speak, let go — your words are
transcribed **on your own PC** and typed into whatever app you're using.

- No account, no telemetry, no cloud audio. The network is used only when you
  download a speech model.
- Hold **Ctrl + Win** (changeable) to dictate. A small black pill shows it's listening.
- Speech models are downloaded from inside the app, never bundled.
- English only for now.

## Download Aural

Get Aural **only** from one of these two official sources:

- **GitHub Releases** of the official repository:
  <https://github.com/Aurathex/Aural/releases>. Download `Aural_<version>_x64-setup.exe`.
- **WinGet**, the official package `Aurathex.Aural` (Windows 10 1809+ / Windows 11):

  ```powershell
  winget install --id Aurathex.Aural -e
  ```

  WinGet downloads the same installer from the GitHub release and refuses it if its
  SHA-256 doesn't match the one in the package manifest.

Copies from anywhere else (mirrors, download sites, "cracked" or "pro" versions,
re-uploads) are not official, and redistributing Aural is not permitted by its license.

The installer sets Aural up for your Windows user only and doesn't need administrator
rights.

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
   when you downloaded it, or you used WinGet as above.
2. **Hash.** Compare the installer's SHA-256 with the line for it in `SHA256SUMS.txt`
   on the same release page:

   ```powershell
   Get-FileHash .\Aural_0.1.0_x64-setup.exe -Algorithm SHA256
   ```

3. **Build provenance.** Each release installer is built by the repository's public
   release workflow, which publishes a signed build attestation. With the
   [GitHub CLI](https://cli.github.com/) you can check that the file you have is exactly
   what that workflow built, and from which commit:

   ```powershell
   gh attestation verify .\Aural_0.1.0_x64-setup.exe --repo Aurathex/Aural
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
| Where speech models come from, with their SHA-256 | `manifests/catalog.v1.json` (Hugging Face URLs pinned to exact commits) |
| What Aural sends over the network (only model downloads) | `PRIVACY.md`, `crates/aural-models/src/download.rs` |
| Keyboard hook, clipboard and text insertion | `crates/aural-platform/src/` |
| Third-party licenses | `THIRD_PARTY_NOTICES.md` |

A prompt you could use: *"Review github.com/Aurathex/Aural at tag v0.1.0. Check what the
installer and uninstaller change on my PC, every network request the app can make,
where speech models are downloaded from and how they are verified, how the keyboard
hook and clipboard are used, and whether the release workflow could ship anything other
than what is in the repository."* An AI review is a useful second look. It is not a
guarantee.

## First run

1. Aural opens on **Models**. Download the recommended model (Parakeet TDT 0.6B v2,
   661 MB; Whisper small.en or base.en for older PCs). Downloads are checked against a
   fixed SHA-256 before use.
2. Click into any app, hold **Ctrl + Win**, speak, let go. The text appears where your
   cursor is.
3. Closing the window keeps Aural in the notification area. Turn on **Start Aural with
   Windows** under General if you want it there after every sign-in.

If something goes wrong the pill says so briefly — for example "Mic blocked" (Windows
privacy settings), "No model", or "Copied — Ctrl+V" when Aural couldn't type into the
window (admin apps, or you switched windows) and left the text on your clipboard.

## Uninstall

Settings → About → **Delete Aural** removes the app, every downloaded model, your
settings and the startup entry (type `YES, DELETE` to confirm). Uninstalling from
Windows Settings → Apps also works; tick "Delete the application data" there to remove
models and settings too.

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
