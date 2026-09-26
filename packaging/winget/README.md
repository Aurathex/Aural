# WinGet packaging

Package identifier: **`Aurathex.Aural`** · installer type `nullsoft` (Tauri NSIS),
per-user (`Scope: user`), silent switch `/S`.

```powershell
winget install --id Aurathex.Aural -e
```

## How manifests are made

WinGet requires the SHA-256 of the exact installer on GitHub Releases, so the manifests
are generated from that file rather than kept in this repository:

```powershell
./packaging/winget/New-WingetManifests.ps1 -Version 0.1.0 `
  -Installer target/release/bundle/nsis/Aural_0.1.0_x64-setup.exe `
  -Repository Djdhmf/Aural -License PolyForm-Noncommercial-1.0.0 -OutDir winget-out
winget validate --manifest winget-out/manifests/a/Aurathex/Aural/0.1.0
```

The `Release` workflow does this automatically and attaches the result.

## Submitting

- **Automated:** run the `Submit to WinGet` workflow (needs a `WINGET_TOKEN` secret, a
  GitHub token with `public_repo` scope). It uses a pinned, hash-checked
  [wingetcreate](https://github.com/microsoft/winget-create) release to validate the
  manifests and open the pull request.
- **By hand:** fork microsoft/winget-pkgs, copy the three YAML files to
  `manifests/a/Aurathex/Aural/<version>/`, and open a pull request.

## Before the first submission

- The installer URL must be public, so the GitHub repository and release must be public.
- The `License` field must match `LICENSE.md` at that tag.
- The WinGet community repository accepts source-available and non-commercial software;
  the manifest only points at our download, it doesn't redistribute it.
- Choose whether "Aurathex" is the publisher name you want permanently: the identifier
  `Aurathex.Aural` can't be renamed cheaply once people have installed it.
