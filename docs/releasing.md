# Releasing Aural

1. **Version.** Set the same version in `app/src-tauri/tauri.conf.json`,
   `app/src-tauri/Cargo.toml` and `app/package.json`. Commit.
2. **Check on a real PC.** Build the installer (`cd app; npx tauri build`), install it
   silently (`Aural_<v>_x64-setup.exe /S`), and go through `docs/testing/manual.md`.
3. **Tag.** `git tag v<version>` and push the tag. The `Release` workflow builds the
   installer, `SHA256SUMS.txt` and `winget-manifests-<version>.zip` and attaches them to
   a **draft** release.
4. **Publish.** Review the draft (download the installer, check the hash), write the
   release notes, publish.
5. **WinGet.** Run the `Submit to WinGet` workflow with the version. It validates the
   manifests and opens a pull request on
   [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs). The first
   submission of `Aurathex.Aural` is reviewed by the WinGet team; later versions are
   usually merged automatically once the validation pipeline passes. Details:
   [packaging/winget/README.md](../packaging/winget/README.md).

## Code signing

Not set up yet. Unsigned installers trigger SmartScreen until the file builds up
reputation, and some antivirus products are suspicious of unsigned apps that use a
keyboard hook and simulated input. Options are in `docs/LICENSING.md` (the SignPath
Foundation's free program requires an OSI license, which Aural's license is not).
