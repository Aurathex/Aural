# WinGet packaging

> **Status: not used.** Aural is not on WinGet; the only official download is
> <https://github.com/Aurathex/Aural/releases>. Submitting was skipped for now (see
> `docs/releasing.md`, step 5). This folder and the `Submit to WinGet` workflow are kept
> for if that changes; the `winget install` command below will not work until then.

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
  -Repository Aurathex/Aural -License PolyForm-Strict-1.0.0 -OutDir winget-out
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
- The WinGet community repository lists proprietary and source-available software, not only open source;
  the manifest only points at our download, it doesn't redistribute it.
- Choose whether "Aurathex" is the publisher name you want permanently: the identifier
  `Aurathex.Aural` can't be renamed cheaply once people have installed it.
