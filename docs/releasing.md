# Releasing Aural

Official home: **<https://github.com/Aurathex/Aural>**. Official WinGet package:
**`Aurathex.Aural`**. The README's "Download Aural" section tells users these are the
only sources to trust; keep it accurate.

## One-time repository setup

Do this once, when `Aurathex/Aural` is created and made public. All of it is free for
public repositories.

- **Settings → Actions → General:** workflow permissions *Read repository contents*.
  The workflows ask for more where they need it (`release.yml`: contents, attestations,
  id-token).
- **Security features.** In *Settings → Code security*, or with the GitHub CLI:

  ```powershell
  $r = 'repos/Aurathex/Aural'
  gh api -X PUT "$r/vulnerability-alerts"               # Dependabot alerts (needs the dependency graph, on by default for public repos)
  gh api -X PUT "$r/automated-security-fixes"           # Dependabot security updates
  gh api -X PUT "$r/private-vulnerability-reporting"    # "Report a vulnerability" button used by SECURITY.md
  '{"security_and_analysis":{"secret_scanning":{"status":"enabled"},"secret_scanning_push_protection":{"status":"enabled"}}}' |
    gh api -X PATCH $r --input -                        # secret scanning + push protection
  ```

  Code scanning runs from `.github/workflows/codeql.yml` (CodeQL for the workflows,
  TypeScript/Svelte and Rust). Dependency review runs on every pull request from
  `.github/workflows/dependency-review.yml`. Dependabot version updates for Cargo, npm
  and GitHub Actions are configured in `.github/dependabot.yml`.
- **Branch protection for `main`:** require pull requests and the `CI` checks.

## Each release

1. **Version.** Set the same version in `app/src-tauri/tauri.conf.json`,
   `app/src-tauri/Cargo.toml` and `app/package.json`. Commit.
2. **Check on a real PC.** Build the installer (`cd app; npx tauri build`) and go through
   `docs/testing/manual.md`. Run the installer checks from an ordinary terminal, not from
   inside another app's sandbox (see that file).
3. **Tag.** `git tag v<version>` and push the tag. The `Release` workflow:
   - refuses to build if `Cargo.lock` is out of date, and installs npm packages exactly
     as locked (`npm ci`);
   - builds the installer and the WinGet manifests;
   - writes `BUILD-INFO.txt` (commit, toolchain and tool versions, lock-file hashes);
   - writes `SHA256SUMS.txt` covering every release file;
   - publishes a GitHub **build attestation** for the installer, so anyone can check with
     `gh attestation verify <file> --repo Aurathex/Aural` that it came from this workflow
     and commit;
   - attaches everything to a **draft** release.
4. **Publish.** Review the draft: download the installer, check it against
   `SHA256SUMS.txt`, run `gh attestation verify` on it, install it, write the release
   notes (include the "not code-signed" paragraph below), publish.
5. **WinGet.** Run the `Submit to WinGet` workflow with the version. It validates the
   manifests and opens a pull request on
   [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs). The first
   submission of `Aurathex.Aural` is reviewed by the WinGet team; later versions are
   usually merged automatically once the validation pipeline passes. Details:
   [packaging/winget/README.md](../packaging/winget/README.md).

## Code signing: not used for the first release

Owner decision, 2026-09-26: the first public release ships **unsigned**. Known
consequences, which the README explains to users:

- SmartScreen will usually show "Windows protected your PC" until the file builds up
  reputation. Users decide whether to continue (More info → Run anyway) after checking
  the source; the README must never tell them to click through blindly.
- Smart App Control (Windows 11) blocks unsigned apps with no override, so Aural can't
  be installed where it is on.
- Antivirus products may hold unsigned programs for analysis. Found on 2026-09-26: with
  Avast Antivirus active, each freshly built `uninstall.exe` was auto-sandboxed (Avast
  `autosandbox.log`: "Autosandbox candidate … Result: Sandboxing", reason `0x00020000`)
  and hung, which blocks uninstall, installer-UI upgrades and Delete Aural until Avast
  releases it. With Avast off, all three passed on the real system.

What stands in for a signature: official download locations only, `SHA256SUMS.txt`,
GitHub build attestations, WinGet's own hash check, and fully public source and
release workflow. Signing options for later are in `docs/LICENSING.md` (the SignPath
Foundation's free program requires an OSI license, which Aural's license is not).

Release-notes paragraph:

> Aural is not code-signed yet. Windows SmartScreen will probably show "Windows
> protected your PC" for this download. Only continue if you downloaded it from
> github.com/Aurathex/Aural/releases or with `winget install --id Aurathex.Aural -e`, and
> ideally after checking its SHA-256 and build attestation (see the README). If you'd
> rather not, don't run it.
