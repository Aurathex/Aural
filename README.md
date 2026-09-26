# Aural

Local voice dictation for Windows. Hold a hotkey, speak, let go — your words are
transcribed **on your own PC** and typed into whatever app you're using.

- No account, no telemetry, no cloud audio. The network is used only when you
  download a speech model.
- Hold **Ctrl + Win** (changeable) to dictate. A small black pill shows it's listening.
- Speech models are downloaded from inside the app, never bundled.
- English only for now.

## Install

With WinGet (Windows 10 1809+ / Windows 11):

```powershell
winget install --id Aurathex.Aural -e
```

Or download `Aural_<version>_x64-setup.exe` from
[GitHub Releases](https://github.com/Djdhmf/Aural/releases) and run it. It installs for
your user only and doesn't need administrator rights. Check the download against
`SHA256SUMS.txt` on the same release page if you like:

```powershell
Get-FileHash .\Aural_0.1.0_x64-setup.exe -Algorithm SHA256
```

The installer isn't code-signed yet, so Windows SmartScreen may show "Windows protected
your PC". Choose **More info → Run anyway** only if you downloaded it from the page
above.

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
